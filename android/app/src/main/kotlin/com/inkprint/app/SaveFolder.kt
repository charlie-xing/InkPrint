package com.inkprint.app

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import android.util.Log
import java.io.File

/** One entry of a folder inside the user's save folder. */
data class DocEntry(
    val uri: Uri,
    val documentId: String,
    val name: String,
    val mimeType: String,
    val sizeBytes: Long,
    val modifiedMs: Long,
) {
    val isFolder: Boolean get() = mimeType == Document.MIME_TYPE_DIR

    /** "1790148775_4_Weekly_Notes.pdf" -> "Weekly Notes.pdf": drops the core's timestamp/job-id prefix. */
    val displayName: String
        get() = if (isFolder) name
        else name.replace(Regex("^\\d+_\\d+_"), "").replace('_', ' ').ifBlank { name }
}

/**
 * The folder the user picked (via the system folder picker) to keep printed
 * files in.
 *
 * Access goes through the Storage Access Framework with a persisted tree
 * grant, so no broad storage permission is needed and the user sees exactly
 * the same files in their reader app. Once a folder is set it is the single
 * home for jobs: incoming files are moved there out of the app's staging
 * directory ([JobStorage.jobDir]).
 */
object SaveFolder {

    private const val TAG = "SaveFolder"
    private const val PREFS = "inkprint"
    private const val KEY_TREE = "save_tree_uri"

    /** Suggested starting point for the picker: where jobs were published before. */
    val INITIAL_URI: Uri = DocumentsContract.buildDocumentUri(
        "com.android.externalstorage.documents",
        "primary:Documents/${JobStorage.FOLDER}"
    )

    /** The chosen tree, or null when none is set or its grant has been revoked. */
    fun treeUri(context: Context): Uri? {
        val saved = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_TREE, null) ?: return null
        val uri = Uri.parse(saved)
        val granted = context.contentResolver.persistedUriPermissions.any {
            it.uri == uri && it.isReadPermission && it.isWritePermission
        }
        return if (granted) uri else null
    }

    /** Remember [uri] (a result of ACTION_OPEN_DOCUMENT_TREE), releasing the previous grant. */
    fun setTree(context: Context, uri: Uri) {
        val resolver = context.contentResolver
        resolver.takePersistableUriPermission(
            uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        )
        resolver.persistedUriPermissions
            .filter { it.uri != uri }
            .forEach {
                resolver.releasePersistableUriPermission(
                    it.uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                )
            }
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putString(KEY_TREE, uri.toString()).apply()
    }

    fun rootDocumentId(tree: Uri): String = DocumentsContract.getTreeDocumentId(tree)

    fun documentUri(tree: Uri, documentId: String): Uri =
        DocumentsContract.buildDocumentUriUsingTree(tree, documentId)

    /** "primary:Documents/InkPrint" -> "Documents/InkPrint"; other providers show their raw id. */
    fun label(tree: Uri): String {
        val id = rootDocumentId(tree)
        return if (id.startsWith("primary:")) id.removePrefix("primary:").ifEmpty { "Internal storage" } else id
    }

    /** Children of [parentId]: folders first by name, then files newest first. */
    fun list(context: Context, tree: Uri, parentId: String): List<DocEntry> {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentId)
        val projection = arrayOf(
            Document.COLUMN_DOCUMENT_ID,
            Document.COLUMN_DISPLAY_NAME,
            Document.COLUMN_MIME_TYPE,
            Document.COLUMN_SIZE,
            Document.COLUMN_LAST_MODIFIED,
        )
        val entries = mutableListOf<DocEntry>()
        context.contentResolver.query(children, projection, null, null, null)?.use { c ->
            while (c.moveToNext()) {
                val id = c.getString(0)
                val name = c.getString(1) ?: continue
                if (name.startsWith(".")) continue
                entries += DocEntry(
                    uri = documentUri(tree, id),
                    documentId = id,
                    name = name,
                    mimeType = c.getString(2) ?: "application/octet-stream",
                    sizeBytes = if (c.isNull(3)) 0 else c.getLong(3),
                    modifiedMs = if (c.isNull(4)) 0 else c.getLong(4),
                )
            }
        }
        return entries.sortedWith(
            compareByDescending<DocEntry> { it.isFolder }
                .thenBy { if (it.isFolder) it.name.lowercase() else "" }
                .thenByDescending { it.modifiedMs }
        )
    }

    fun createFolder(context: Context, tree: Uri, parentId: String, name: String): Uri? =
        DocumentsContract.createDocument(
            context.contentResolver, documentUri(tree, parentId), Document.MIME_TYPE_DIR, name
        )

    fun rename(context: Context, doc: Uri, newName: String): Uri? =
        DocumentsContract.renameDocument(context.contentResolver, doc, newName)

    fun delete(context: Context, doc: Uri): Boolean =
        DocumentsContract.deleteDocument(context.contentResolver, doc)

    /**
     * Move [entry] from [fromParentId] into [toParentId]. Uses the provider's
     * native move when it supports one, otherwise copies and deletes.
     */
    fun move(context: Context, tree: Uri, entry: DocEntry, fromParentId: String, toParentId: String): Uri? {
        val resolver = context.contentResolver
        val target = documentUri(tree, toParentId)
        try {
            DocumentsContract.moveDocument(resolver, entry.uri, documentUri(tree, fromParentId), target)
                ?.let { return it }
        } catch (e: Exception) {
            Log.i(TAG, "Native move unsupported, copying instead: ${e.message}")
        }
        if (entry.isFolder) return null
        val copy = DocumentsContract.createDocument(resolver, target, entry.mimeType, entry.name) ?: return null
        resolver.openInputStream(entry.uri)?.use { input ->
            resolver.openOutputStream(copy)?.use { output -> input.copyTo(output) }
        } ?: return null
        DocumentsContract.deleteDocument(resolver, entry.uri)
        return copy
    }

    /** Copy [file] into the top of the save folder. Returns the new document, or null on failure. */
    fun importFile(context: Context, tree: Uri, file: File): Uri? {
        val resolver = context.contentResolver
        val doc = DocumentsContract.createDocument(
            resolver, documentUri(tree, rootDocumentId(tree)), JobStorage.mimeTypeOf(file), file.name
        ) ?: return null
        try {
            resolver.openOutputStream(doc)?.use { out ->
                file.inputStream().use { it.copyTo(out) }
            } ?: throw IllegalStateException("openOutputStream returned null")
        } catch (e: Exception) {
            DocumentsContract.deleteDocument(resolver, doc)
            throw e
        }
        return doc
    }

    /**
     * Move everything still in the staging directory into the save folder —
     * jobs received before a folder was chosen. A file whose name already
     * exists at the top of the folder (typically the copy published to
     * Documents/InkPrint earlier) is not copied twice.
     */
    fun adoptStagedJobs(context: Context, tree: Uri) {
        val existing = list(context, tree, rootDocumentId(tree)).map { it.name }.toSet()
        JobStorage.listJobs(context).forEach { file ->
            try {
                if (file.name in existing || importFile(context, tree, file) != null) file.delete()
            } catch (e: Exception) {
                Log.w(TAG, "Could not move ${file.name} into the save folder: ${e.message}")
            }
        }
    }
}
