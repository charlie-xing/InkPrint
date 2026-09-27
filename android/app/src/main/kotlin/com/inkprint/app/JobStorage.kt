package com.inkprint.app

import android.content.ContentValues
import android.content.Context
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.util.Log
import java.io.File

/**
 * Where print jobs live.
 *
 * The Rust IPP core writes files through a plain filesystem path, so incoming
 * jobs always land in the app-specific external directory — no storage
 * permission is involved and it works on every supported API level.
 *
 * Once the user has picked a save folder ([SaveFolder]), each finished job is
 * moved there and the app directory is only a staging area.
 *
 * Until then, to keep the PDFs reachable from the device's own reader app,
 * each finished job is published into the shared `Documents/InkPrint/`
 * collection: via MediaStore on API 29+, and with a direct copy (guarded by
 * the legacy write permission) on API 28 and below. Publishing is best-effort
 * — a failure there never loses the job, which stays in the app directory
 * either way.
 */
object JobStorage {

    private const val TAG = "JobStorage"
    const val FOLDER = "InkPrint"

    /** Directory the Rust core writes incoming jobs into. */
    fun jobDir(context: Context): File {
        val base = context.getExternalFilesDir(null) ?: context.filesDir
        return File(base, FOLDER).also { it.mkdirs() }
    }

    fun listJobs(context: Context): List<File> =
        jobDir(context).listFiles()
            ?.filter { it.isFile }
            ?.sortedByDescending { it.lastModified() }
            ?: emptyList()

    /**
     * Hand a finished job over to where the user keeps their files. Returns a
     * Uri readers can open: the job's document in the save folder, or null when
     * the job stays in the app directory (no folder set, or moving it failed).
     */
    fun deliver(context: Context, file: File): Uri? {
        val tree = SaveFolder.treeUri(context)
        if (tree == null) {
            publishToDocuments(context, file)
            return null
        }
        return try {
            SaveFolder.importFile(context, tree, file)?.also { file.delete() }
        } catch (e: Exception) {
            Log.w(TAG, "Could not move ${file.name} into the save folder: ${e.message}")
            null
        }
    }

    /**
     * Copy [file] into the shared Documents/InkPrint/ collection so other apps
     * (the BOOX reader, a file manager) can see it. Returns the published Uri,
     * or null when publishing was not possible.
     */
    fun publishToDocuments(context: Context, file: File): Uri? = try {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            publishViaMediaStore(context, file)
        } else {
            publishViaLegacyCopy(context, file)
        }
    } catch (e: Exception) {
        Log.w(TAG, "Could not publish ${file.name} to shared storage: ${e.message}")
        null
    }

    private fun publishViaMediaStore(context: Context, file: File): Uri? {
        val resolver = context.contentResolver
        val collection = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, file.name)
            put(MediaStore.MediaColumns.MIME_TYPE, mimeTypeOf(file))
            put(MediaStore.MediaColumns.RELATIVE_PATH, "${Environment.DIRECTORY_DOCUMENTS}/$FOLDER")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }

        val uri = resolver.insert(collection, values) ?: return null
        try {
            resolver.openOutputStream(uri)?.use { out ->
                file.inputStream().use { it.copyTo(out) }
            } ?: throw IllegalStateException("openOutputStream returned null")
        } catch (e: Exception) {
            resolver.delete(uri, null, null)
            throw e
        }

        resolver.update(uri, ContentValues().apply {
            put(MediaStore.MediaColumns.IS_PENDING, 0)
        }, null, null)
        return uri
    }

    private fun publishViaLegacyCopy(context: Context, file: File): Uri? {
        @Suppress("DEPRECATION")
        val dir = File(
            Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS),
            FOLDER
        )
        if (!dir.exists() && !dir.mkdirs()) return null

        val target = uniqueTarget(dir, file.name)
        file.inputStream().use { input ->
            target.outputStream().use { output -> input.copyTo(output) }
        }
        // Make the copy visible to other apps without a reboot.
        MediaScannerConnection.scanFile(context, arrayOf(target.absolutePath), null, null)
        return Uri.fromFile(target)
    }

    private fun uniqueTarget(dir: File, name: String): File {
        val base = name.substringBeforeLast('.', name)
        val ext = name.substringAfterLast('.', "")
        var candidate = File(dir, name)
        var n = 1
        while (candidate.exists()) {
            val suffix = if (ext.isEmpty()) "" else ".$ext"
            candidate = File(dir, "$base ($n)$suffix")
            n++
        }
        return candidate
    }

    fun mimeTypeOf(file: File): String = when (file.extension.lowercase()) {
        "pdf" -> "application/pdf"
        "ps" -> "application/postscript"
        "jpg", "jpeg" -> "image/jpeg"
        "png" -> "image/png"
        else -> "application/octet-stream"
    }
}
