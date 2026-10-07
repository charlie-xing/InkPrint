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
import uniffi.inkprint.releaseModels
import java.io.File
import java.util.Collections
import java.util.concurrent.Executors

/**
 * Turns PDFs received by the EPUB printer into EPUBs, one at a time, in the
 * background.
 *
 * A job's PDF is moved into [convertDir] when it is queued and stays there
 * until it is done, so a PDF found in that directory is a job still to do:
 * [resume] picks those up after the process was killed. The finished EPUB
 * (and, if wanted, the PDF) is then handed to [JobStorage.deliver] like any
 * other job. When conversion fails or is cancelled, the PDF is delivered
 * instead so nothing is lost.
 */
object ConversionQueue {

    private const val TAG = "ConversionQueue"
    const val CHANNEL_ID = "inkprint_convert"
    private const val NOTIFICATION_ID = 2
    /** Free the layout/OCR models after this long without work. */
    private const val IDLE_RELEASE_MS = 5 * 60_000L
    /** ONNX Runtime threads: leaves cores for the reader and the system. */
    private const val THREADS = 4u

    data class Status(val name: String, val done: Int, val total: Int, val waiting: Int)

    private val _status = MutableStateFlow<Status?>(null)
    /** The conversion in progress, for the UI; null when idle. */
    val status: StateFlow<Status?> = _status

    private val executor = Executors.newSingleThreadExecutor { r ->
        Thread(r, "epub-convert").apply { priority = Thread.MIN_PRIORITY }
    }
    private val queued = Collections.synchronizedSet(mutableSetOf<String>())
    private val main = Handler(Looper.getMainLooper())
    private val releaseIdleModels = Runnable { executor.execute { releaseModels() } }

    fun convertDir(context: Context): File {
        val base = context.getExternalFilesDir(null) ?: context.filesDir
        return File(base, "convert").also { it.mkdirs() }
    }

    /** Takes over a PDF the EPUB printer just received. */
    fun enqueue(context: Context, pdf: File) {
        val staged = moveTo(pdf, convertDir(context))
        submit(context.applicationContext, staged)
    }

    /** Re-queues jobs left unfinished when the process last died. */
    fun resume(context: Context) {
        convertDir(context).listFiles { f -> f.isFile && f.extension.equals("pdf", true) }
            ?.sortedBy { it.lastModified() }
            ?.forEach { submit(context.applicationContext, it) }
    }

    /** Stops the running conversion after its current page; its PDF is delivered. */
    fun cancelCurrent() = cancelConversion()

    private fun submit(context: Context, pdf: File) {
        if (!queued.add(pdf.absolutePath)) return
        main.removeCallbacks(releaseIdleModels)
        _status.value?.let { _status.value = it.copy(waiting = queued.size - 1) }
        executor.execute {
            try {
                convert(context, pdf)
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

    private fun convert(context: Context, pdf: File) {
        val name = displayName(pdf.name)
        val out = File(convertDir(context), pdf.nameWithoutExtension + ".epub")
        val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val wake = (context.getSystemService(Context.POWER_SERVICE) as PowerManager)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "InkPrint:convert")
        wake.acquire(2 * 60 * 60 * 1000L)
        try {
            report(context, nm, name, 0, 0)
            val options = ConversionOptions(
                title = name,
                pdfiumLib = "libpdfium.so",
                ortLib = "libonnxruntime.so",
                modelsDir = installModels(context)?.absolutePath,
                ocr = Settings.ocr(context),
                threads = THREADS,
            )
            val progress = object : ConversionProgress {
                override fun onPage(done: UInt, total: UInt) =
                    report(context, nm, name, done.toInt(), total.toInt())
            }
            val stats = convertPdfToEpub(pdf.absolutePath, out.absolutePath, options, progress)
            Log.i(TAG, "Converted $name: $stats")
            val epub = moveTo(out, JobStorage.jobDir(context))
            val uri = JobStorage.deliver(context, epub) ?: fileUri(context, epub)
            if (Settings.keepPdf(context)) {
                JobStorage.deliver(context, moveTo(pdf, JobStorage.jobDir(context)))
            } else {
                pdf.delete()
            }
            notifyResult(context, nm, name, success(stats), uri, "application/epub+zip")
        } catch (e: ConvertException.Cancelled) {
            Log.i(TAG, "Conversion of $name cancelled")
            deliverPdfInstead(context, nm, pdf, name, "EPUB conversion cancelled — saved the PDF")
        } catch (e: Exception) {
            Log.e(TAG, "Conversion of $name failed", e)
            deliverPdfInstead(context, nm, pdf, name, "EPUB conversion failed — saved the PDF")
        } finally {
            nm.cancel(NOTIFICATION_ID)
            out.delete()
            if (wake.isHeld) wake.release()
        }
    }

    private fun success(stats: ConversionStats): String {
        val pages = "${stats.pages} page" + if (stats.pages == 1u) "" else "s"
        return when {
            stats.ocrPages > 0u -> "EPUB ready · $pages, ${stats.ocrPages} recognised by OCR"
            stats.imagePages > 0u -> "EPUB ready · $pages, ${stats.imagePages} kept as pictures"
            else -> "EPUB ready · $pages"
        }
    }

    private fun deliverPdfInstead(context: Context, nm: NotificationManager, pdf: File, name: String, message: String) {
        if (!pdf.exists()) return
        val kept = moveTo(pdf, JobStorage.jobDir(context))
        val uri = JobStorage.deliver(context, kept) ?: fileUri(context, kept)
        notifyResult(context, nm, name, message, uri, "application/pdf")
    }

    private fun report(context: Context, nm: NotificationManager, name: String, done: Int, total: Int) {
        _status.value = Status(name, done, total, (queued.size - 1).coerceAtLeast(0))
        val cancel = PendingIntent.getService(
            context, 0,
            Intent(context, PrinterService::class.java).setAction(PrinterService.ACTION_CANCEL_CONVERSION),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val text = if (total == 0) "Preparing…" else "Page $done of $total"
        nm.notify(
            NOTIFICATION_ID,
            NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.ic_popup_sync)
                .setContentTitle("Converting “$name” to EPUB")
                .setContentText(text)
                .setProgress(total, done, total == 0)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .addAction(android.R.drawable.ic_delete, "Cancel", cancel)
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
