package com.inkprint.app

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.FileProvider
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import uniffi.inkprint.ConversionOptions
import uniffi.inkprint.ConversionProgress
import uniffi.inkprint.ConversionStats
import uniffi.inkprint.ConvertException
import uniffi.inkprint.cancelConversion
import uniffi.inkprint.convertPdfToEpub
import uniffi.inkprint.makePdfSearchable
import uniffi.inkprint.releaseModels
import java.io.File
import java.util.Collections
import java.util.concurrent.Executors

/**
 * Background work on received PDFs, one job at a time: turning them into
 * EPUBs (the EPUB printer) or adding an OCR text layer to their scanned
 * pages (the PDF printer with searchable scans on).
 *
 * A job's PDF is moved into its kind's directory ([Kind.dir]) when it is
 * queued and stays there until it is done, so a PDF found there is a job
 * still to do: [resume] picks those up after the process was killed. The
 * result (and, for EPUBs, the PDF if wanted) is then handed to
 * [JobStorage.deliver] like any other job. When the work fails or is
 * cancelled, the PDF as received is delivered instead so nothing is lost.
 */
object ConversionQueue {

    private const val TAG = "ConversionQueue"
    const val CHANNEL_ID = "inkprint_convert"
    private const val NOTIFICATION_ID = 2
    /** Free the layout/OCR models after this long without work. */
    private const val IDLE_RELEASE_MS = 5 * 60_000L
    /** ONNX Runtime threads: leaves cores for the reader and the system. */
    private const val THREADS = 4u

    enum class Kind(val dir: String) { EPUB("convert"), SEARCHABLE("ocr") }

    data class Status(val kind: Kind, val name: String, val done: Int, val total: Int, val waiting: Int)

    private val _status = MutableStateFlow<Status?>(null)
    /** The conversion in progress, for the UI; null when idle. */
    val status: StateFlow<Status?> = _status

    private val executor = Executors.newSingleThreadExecutor { r ->
        Thread(r, "epub-convert").apply { priority = Thread.MIN_PRIORITY }
    }
    private val queued = Collections.synchronizedSet(mutableSetOf<String>())
    private val main = Handler(Looper.getMainLooper())
    private val releaseIdleModels = Runnable { executor.execute { releaseModels() } }

    fun dir(context: Context, kind: Kind): File {
        val base = context.getExternalFilesDir(null) ?: context.filesDir
        return File(base, kind.dir).also { it.mkdirs() }
    }

    /** Takes over a received PDF: [Kind.EPUB] converts it, [Kind.SEARCHABLE] adds a text layer. */
    fun enqueue(context: Context, pdf: File, kind: Kind) {
        val staged = moveTo(pdf, dir(context, kind))
        submit(context.applicationContext, staged, kind)
    }

    /** Re-queues jobs left unfinished when the process last died. */
    fun resume(context: Context) {
        for (kind in Kind.entries) {
            dir(context, kind).listFiles { f -> f.isFile && f.extension.equals("pdf", true) }
                ?.sortedBy { it.lastModified() }
                ?.forEach { submit(context.applicationContext, it, kind) }
        }
    }

    /** Stops the running conversion after its current page; its PDF is delivered. */
    fun cancelCurrent() = cancelConversion()

    private fun submit(context: Context, pdf: File, kind: Kind) {
        if (!queued.add(pdf.absolutePath)) return
        main.removeCallbacks(releaseIdleModels)
        _status.value?.let { _status.value = it.copy(waiting = queued.size - 1) }
        executor.execute {
            try {
                when (kind) {
                    Kind.EPUB -> convert(context, pdf)
                    Kind.SEARCHABLE -> makeSearchable(context, pdf)
                }
            } finally {
                queued.remove(pdf.absolutePath)
                if (queued.isEmpty()) {
                    _status.value = null
                    main.postDelayed(releaseIdleModels, IDLE_RELEASE_MS)
                }
                context.sendBroadcast(
                    Intent(PrinterService.BROADCAST_JOB_RECEIVED).setPackage(context.packageName)
                )
            }
        }
    }

    private fun options(context: Context, title: String, ocr: Boolean) = ConversionOptions(
        title = title,
        pdfiumLib = "libpdfium.so",
        ortLib = "libonnxruntime.so",
        modelsDir = installModels(context)?.absolutePath,
        ocr = ocr,
        threads = THREADS,
    )

    private fun progressFor(context: Context, nm: NotificationManager, kind: Kind, name: String) =
        object : ConversionProgress {
            override fun onPage(done: UInt, total: UInt) =
                report(context, nm, kind, name, done.toInt(), total.toInt())
        }

    private fun wakeLock(context: Context): PowerManager.WakeLock =
        (context.getSystemService(Context.POWER_SERVICE) as PowerManager)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "InkPrint:convert")
            .also { it.acquire(2 * 60 * 60 * 1000L) }

    private fun convert(context: Context, pdf: File) {
        val name = displayName(pdf.name)
        val out = File(dir(context, Kind.EPUB), pdf.nameWithoutExtension + ".epub")
        val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val wake = wakeLock(context)
        try {
            report(context, nm, Kind.EPUB, name, 0, 0)
            val options = options(context, name, Settings.ocr(context))
            val progress = progressFor(context, nm, Kind.EPUB, name)
            val stats = convertPdfToEpub(pdf.absolutePath, out.absolutePath, options, progress)
            Log.i(TAG, "Converted $name: $stats")
            val epub = moveTo(out, JobStorage.jobDir(context))
            val uri = JobStorage.deliver(context, epub) ?: fileUri(context, epub)
            if (Settings.keepPdf(context)) {
                JobStorage.deliver(context, moveTo(pdf, JobStorage.jobDir(context)))
            } else {
                pdf.delete()
            }
            notifyResult(context, nm, name, success(context, stats), uri, "application/epub+zip")
        } catch (e: ConvertException.Cancelled) {
            Log.i(TAG, "Conversion of $name cancelled")
            deliverPdfInstead(context, nm, pdf, name, Lang.string(context, R.string.result_epub_cancelled))
        } catch (e: Exception) {
            Log.e(TAG, "Conversion of $name failed", e)
            deliverPdfInstead(context, nm, pdf, name, Lang.string(context, R.string.result_epub_failed))
        } finally {
            nm.cancel(NOTIFICATION_ID)
            out.delete()
            if (wake.isHeld) wake.release()
        }
    }

    /** Adds an OCR text layer to the scanned pages of a PDF the PDF printer received. */
    private fun makeSearchable(context: Context, pdf: File) {
        val name = displayName(pdf.name)
        val out = File(context.cacheDir, "ocr-" + pdf.name)
        val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val wake = wakeLock(context)
        try {
            report(context, nm, Kind.SEARCHABLE, name, 0, 0)
            val stats = makePdfSearchable(
                pdf.absolutePath, out.absolutePath, options(context, name, true),
                progressFor(context, nm, Kind.SEARCHABLE, name)
            )
            Log.i(TAG, "Text layer for $name: $stats")
            val text = if (stats.ocrPages > 0u) {
                // Same name, so the reader keeps it in place of the original.
                out.copyTo(pdf, overwrite = true)
                Lang.string(context, R.string.result_searchable, stats.ocrPages.toInt(), stats.pages.toInt())
            } else {
                Lang.string(context, R.string.result_searchable_none)
            }
            val kept = moveTo(pdf, JobStorage.jobDir(context))
            val uri = JobStorage.deliver(context, kept) ?: fileUri(context, kept)
            notifyResult(context, nm, name, text, uri, "application/pdf")
        } catch (e: ConvertException.Cancelled) {
            Log.i(TAG, "Text layer for $name cancelled")
            deliverPdfInstead(context, nm, pdf, name, Lang.string(context, R.string.result_ocr_cancelled))
        } catch (e: Exception) {
            Log.e(TAG, "Text layer for $name failed", e)
            deliverPdfInstead(context, nm, pdf, name, Lang.string(context, R.string.result_ocr_failed))
        } finally {
            nm.cancel(NOTIFICATION_ID)
            out.delete()
            if (wake.isHeld) wake.release()
        }
    }

    private fun success(ctx: Context, stats: ConversionStats): String {
        return when {
            stats.ocrPages > 0u -> Lang.string(ctx, R.string.result_epub_ocr, stats.pages.toInt(), stats.ocrPages.toInt())
            stats.imagePages > 0u -> Lang.string(ctx, R.string.result_epub_pictures, stats.pages.toInt(), stats.imagePages.toInt())
            else -> Lang.string(ctx, R.string.result_epub, stats.pages.toInt())
        }
    }

    private fun deliverPdfInstead(context: Context, nm: NotificationManager, pdf: File, name: String, message: String) {
        if (!pdf.exists()) return
        val kept = moveTo(pdf, JobStorage.jobDir(context))
        val uri = JobStorage.deliver(context, kept) ?: fileUri(context, kept)
        notifyResult(context, nm, name, message, uri, "application/pdf")
    }

    /** When progress was last shown: e-ink redraws are slow, so at most every 2 s. */
    private var lastReport = 0L

    private fun report(context: Context, nm: NotificationManager, kind: Kind, name: String, done: Int, total: Int) {
        val now = android.os.SystemClock.elapsedRealtime()
        if (done != 0 && done != total && now - lastReport < 2000) return
        lastReport = now
        _status.value = Status(kind, name, done, total, (queued.size - 1).coerceAtLeast(0))
        val cancel = PendingIntent.getService(
            context, 0,
            Intent(context, PrinterService::class.java).setAction(PrinterService.ACTION_CANCEL_CONVERSION),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val text = if (total == 0) Lang.string(context, R.string.progress_preparing)
            else Lang.string(context, R.string.progress_page, done, total)
        nm.notify(
            NOTIFICATION_ID,
            NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.ic_popup_sync)
                .setContentTitle(
                    when (kind) {
                        Kind.EPUB -> Lang.string(context, R.string.progress_converting, name)
                        Kind.SEARCHABLE -> Lang.string(context, R.string.progress_recognising, name)
                    }
                )
                .setContentText(text)
                .setProgress(total, done, total == 0)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .addAction(android.R.drawable.ic_delete, Lang.string(context, R.string.cancel), cancel)
                .build()
        )
    }

    private fun notifyResult(context: Context, nm: NotificationManager, name: String, text: String, uri: Uri, mime: String) {
        val open = PendingIntent.getActivity(
            context, name.hashCode(),
            Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        nm.notify(
            NOTIFICATION_ID + 1000 + (name.hashCode() and 0xFFFF),
            NotificationCompat.Builder(context, PrinterService.CHANNEL_ID_JOBS)
                .setSmallIcon(android.R.drawable.ic_menu_save)
                .setContentTitle(name)
                .setContentText(text)
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
        )
    }

    private fun fileUri(context: Context, file: File): Uri =
        FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)

    /**
     * Copies the bundled layout/OCR models out of the APK once per app
     * version (ONNX Runtime wants files). Null when this build has none.
     */
    private fun installModels(context: Context): File? {
        if (!BuildConfig.EPUB) return null
        val names = context.assets.list("models")
            ?.filter { it.endsWith(".onnx") || it.endsWith(".txt") }
            .orEmpty()
        if (names.isEmpty()) return null
        val dir = File(context.filesDir, "models").also { it.mkdirs() }
        val stamp = File(dir, ".version")
        val version = BuildConfig.VERSION_CODE.toString()
        val current = stamp.exists() && stamp.readText() == version && names.all { File(dir, it).exists() }
        if (!current) {
            for (n in names) {
                val tmp = File(dir, "$n.tmp")
                context.assets.open("models/$n").use { input -> tmp.outputStream().use { input.copyTo(it) } }
                tmp.renameTo(File(dir, n))
            }
            stamp.writeText(version)
        }
        return dir
    }

    /** Moves [file] into [dir] (same volume: a rename), returning the new file. */
    private fun moveTo(file: File, dir: File): File {
        val target = File(dir, file.name)
        if (file.absolutePath == target.absolutePath) return file
        if (!file.renameTo(target)) {
            file.copyTo(target, overwrite = true)
            file.delete()
        }
        return target
    }

    /** "1790148775_4_Weekly_Notes.pdf" → "Weekly Notes" */
    private fun displayName(fileName: String): String =
        fileName.substringBeforeLast('.')
            .replace(Regex("^\\d+_\\d+_"), "")
            .replace('_', ' ')
            .trim()
            .ifBlank { "Untitled" }
}
