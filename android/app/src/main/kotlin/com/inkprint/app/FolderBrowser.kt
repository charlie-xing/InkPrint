package com.inkprint.app

import android.content.Context
import android.net.Uri
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.text.SimpleDateFormat
import java.util.*

/** A folder on the navigation path: its document id and the name shown in the breadcrumb. */
private data class Crumb(val id: String, val name: String)

/** Pending dialog of the folder browser. */
private sealed interface BrowserDialog {
    data object NewFolder : BrowserDialog
    data class Actions(val entry: DocEntry) : BrowserDialog
    data class Rename(val entry: DocEntry) : BrowserDialog
    data class Move(val entry: DocEntry) : BrowserDialog
    data class Delete(val entry: DocEntry) : BrowserDialog
}

// ── Without a save folder: the app's own directory ────────────────────────────

/** Received files in the app directory, a page at a time, with a prompt to pick a save folder. */
@Composable
fun FileList(
    files: List<FileEntry>,
    pageSize: Int,
    onOpen: (String) -> Unit,
    onDelete: (List<FileEntry>) -> Unit,
    onChooseFolder: () -> Unit,
) {
    val dateFmt = remember { SimpleDateFormat("MM/dd HH:mm", Locale.getDefault()) }
    var page by remember { mutableStateOf(0) }
    val pages = maxOf(1, (files.size + pageSize - 1) / pageSize)
    if (page >= pages) page = 0
    var selecting by remember { mutableStateOf(false) }
    var selected by remember { mutableStateOf(setOf<String>()) }
    var confirm by remember { mutableStateOf<Pair<List<FileEntry>, Boolean>?>(null) }
    // Files that went away (deleted, delivered elsewhere) drop out of the selection
    val present = files.map { it.path }.toSet()
    if (!present.containsAll(selected)) selected = selected intersect present

    FilesHeader(stringResource(R.string.printed_files), files.size)
    InkCard {
        Text(stringResource(R.string.choose_folder_prompt), fontSize = 15.sp)
        InkButton(stringResource(R.string.choose_folder), onClick = onChooseFolder, modifier = Modifier.fillMaxWidth())
    }
    if (files.isEmpty()) {
        EmptyNote(stringResource(R.string.no_files_yet))
    } else {
        SelectionBar(
            selecting = selecting,
            selected = selected.size,
            all = selected.size == files.size,
            onSelect = { selecting = true },
            onClear = { confirm = files to true },
            onToggleAll = { selected = if (selected.size == files.size) emptySet() else present },
            onDelete = { confirm = files.filter { it.path in selected } to false },
            onDone = { selecting = false; selected = emptySet() },
        )
        files.drop(page * pageSize).take(pageSize).forEach { f ->
            EntryRow(
                tag = fileTag(f.name),
                name = f.displayName,
                subtitle = "${dateFmt.format(Date(f.modifiedMs))} · ${formatSize(f.sizeBytes)}",
                onClick = {
                    if (selecting) selected = selected.toggle(f.path) else onOpen(f.path)
                },
                onMore = null,
                checked = if (selecting) f.path in selected else null,
            )
        }
        Pager(page, pages) { page = it }
    }

    confirm?.let { (victims, clear) ->
        ConfirmDelete(
            text = if (clear) stringResource(R.string.clear_text, victims.size)
                else stringResource(R.string.delete_selected_text, victims.size),
            onDismiss = { confirm = null },
            onConfirm = {
                confirm = null
                selecting = false
                selected = emptySet()
                onDelete(victims)
            }
        )
    }
}

// ── With a save folder ───────────────────────────────────────────────────────

/**
 * Browses and manages the user's save folder: walk into subfolders, create
 * folders, and open, share, rename, move or delete entries.
 */
@Composable
fun FolderBrowser(
    tree: Uri,
    refreshTick: Int,
    pageSize: Int,
    onOpen: (DocEntry) -> Unit,
    onShare: (DocEntry) -> Unit,
    onChangeFolder: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val dateFmt = remember { SimpleDateFormat("MM/dd HH:mm", Locale.getDefault()) }

    var path by remember(tree) { mutableStateOf(listOf(Crumb(SaveFolder.rootDocumentId(tree), SaveFolder.label(tree)))) }
    val current = path.last()
    var reload by remember { mutableStateOf(0) }
    var page by remember(tree, current) { mutableStateOf(0) }
    var dialog by remember { mutableStateOf<BrowserDialog?>(null) }
    var selecting by remember(tree, current) { mutableStateOf(false) }
    var selected by remember(tree, current) { mutableStateOf(setOf<String>()) }
    /** Entries waiting for the user to confirm their deletion, and whether that is "Clear". */
    var confirm by remember { mutableStateOf<Pair<List<DocEntry>, Boolean>?>(null) }

    // null while loading; a failure (folder deleted, grant lost) shows as an error line
    val listing by produceState<Result<List<DocEntry>>?>(null, tree, current, refreshTick, reload) {
        value = withContext(Dispatchers.IO) {
            runCatching { SaveFolder.list(context, tree, current.id) }
        }
    }
    val entries = listing?.getOrNull().orEmpty()

    /** Run a storage operation off the main thread, then refresh the listing. */
    fun runOp(failure: String, op: () -> Any?) {
        scope.launch {
            val ok = withContext(Dispatchers.IO) {
                runCatching { op() }.map { it != null && it != false }.getOrDefault(false)
            }
            if (!ok) Toast.makeText(context, failure, Toast.LENGTH_SHORT).show()
            reload++
        }
    }

    val pages = maxOf(1, (entries.size + pageSize - 1) / pageSize)
    if (page >= pages) page = 0
    val present = entries.map { it.documentId }.toSet()
    if (!present.containsAll(selected)) selected = selected intersect present
    val files = entries.filter { !it.isFolder }

    /** Delete [victims] off the main thread, report what failed, then refresh. */
    fun deleteAll(victims: List<DocEntry>, failure: String) {
        scope.launch {
            val failed = withContext(Dispatchers.IO) {
                victims.count { e -> !runCatching { SaveFolder.delete(context, e.uri) }.getOrDefault(false) }
            }
            if (failed > 0) Toast.makeText(context, failure, Toast.LENGTH_SHORT).show()
            reload++
        }
    }

    FilesHeader(stringResource(R.string.printed_files), entries.size)
    // Where we are; tap a segment to go back up to it.
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        path.forEachIndexed { i, crumb ->
            if (i > 0) Text(" / ", fontSize = 15.sp)
            val last = i == path.lastIndex
            Text(
                crumb.name,
                fontSize = 15.sp,
                fontWeight = if (last) FontWeight.Bold else FontWeight.Normal,
                style = if (last) TextStyle.Default else TextStyle(textDecoration = androidx.compose.ui.text.style.TextDecoration.Underline),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false).tap(enabled = !last) { path = path.take(i + 1) }
            )
        }
    }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        if (path.size > 1) {
            InkButton(stringResource(R.string.up), onClick = { path = path.dropLast(1) }, height = 48.dp)
        }
        InkButton(stringResource(R.string.new_folder), onClick = { dialog = BrowserDialog.NewFolder }, height = 48.dp, modifier = Modifier.weight(1f))
        InkButton(stringResource(R.string.change_folder), onClick = onChangeFolder, height = 48.dp, modifier = Modifier.weight(1f))
    }

    when {
        listing == null -> EmptyNote(stringResource(R.string.loading))
        listing!!.isFailure -> EmptyNote(stringResource(R.string.folder_unreadable))
        entries.isEmpty() -> EmptyNote(
            stringResource(if (path.size == 1) R.string.no_files_yet else R.string.folder_empty)
        )
        else -> {
            SelectionBar(
                selecting = selecting,
                selected = selected.size,
                all = selected.size == entries.size,
                onSelect = { selecting = true },
                onClear = if (files.isEmpty()) null else ({ confirm = files to true }),
                onToggleAll = { selected = if (selected.size == entries.size) emptySet() else present },
                onDelete = { confirm = entries.filter { it.documentId in selected } to false },
                onDone = { selecting = false; selected = emptySet() },
            )
            entries.drop(page * pageSize).take(pageSize).forEach { entry ->
                EntryRow(
                    tag = if (entry.isFolder) stringResource(R.string.tag_folder) else fileTag(entry.name),
                    name = entry.displayName,
                    subtitle = if (entry.isFolder) stringResource(R.string.folder)
                        else "${dateFmt.format(Date(entry.modifiedMs))} · ${formatSize(entry.sizeBytes)}",
                    onClick = {
                        when {
                            selecting -> selected = selected.toggle(entry.documentId)
                            entry.isFolder -> path = path + Crumb(entry.documentId, entry.name)
                            else -> onOpen(entry)
                        }
                    },
                    onMore = if (selecting) null else ({ dialog = BrowserDialog.Actions(entry) }),
                    checked = if (selecting) entry.documentId in selected else null,
                )
            }
            Pager(page, pages) { page = it }
        }
    }

    confirm?.let { (victims, clear) ->
        val failed = stringResource(R.string.delete_failed)
        val folders = victims.count { it.isFolder }
        ConfirmDelete(
            text = when {
                clear -> stringResource(R.string.clear_text, victims.size)
                victims.size == 1 && folders == 1 -> stringResource(R.string.delete_folder_text, victims[0].name)
                else -> stringResource(R.string.delete_selected_text, victims.size)
            },
            onDismiss = { confirm = null },
            onConfirm = {
                confirm = null
                selecting = false
                selected = emptySet()
                deleteAll(victims, failed)
            }
        )
    }

    when (val d = dialog) {
        null -> {}
        is BrowserDialog.Actions -> InkDialog(
            title = d.entry.displayName,
            onDismiss = { dialog = null },
            buttons = { InkButton(stringResource(R.string.cancel), onClick = { dialog = null }, modifier = Modifier.fillMaxWidth()) }
        ) {
            if (!d.entry.isFolder) {
                InkButton(stringResource(R.string.share), onClick = { dialog = null; onShare(d.entry) }, modifier = Modifier.fillMaxWidth())
            }
            InkButton(stringResource(R.string.rename), onClick = { dialog = BrowserDialog.Rename(d.entry) }, modifier = Modifier.fillMaxWidth())
            InkButton(stringResource(R.string.move_to), onClick = { dialog = BrowserDialog.Move(d.entry) }, modifier = Modifier.fillMaxWidth())
            InkButton(stringResource(R.string.delete), onClick = { dialog = BrowserDialog.Delete(d.entry) }, modifier = Modifier.fillMaxWidth())
        }
        BrowserDialog.NewFolder -> {
            val failed = stringResource(R.string.create_folder_failed)
            NameDialog(
                title = stringResource(R.string.new_folder),
                initial = "",
                confirmLabel = stringResource(R.string.create),
                onDismiss = { dialog = null },
                onConfirm = { name ->
                    dialog = null
                    runOp(failed) { SaveFolder.createFolder(context, tree, current.id, name) }
                }
            )
        }
        is BrowserDialog.Rename -> {
            val failed = stringResource(R.string.rename_failed)
            NameDialog(
                title = stringResource(R.string.rename),
                initial = d.entry.name,
                confirmLabel = stringResource(R.string.rename),
                onDismiss = { dialog = null },
                onConfirm = { name ->
                    dialog = null
                    runOp(failed) { SaveFolder.rename(context, d.entry.uri, name) }
                }
            )
        }
        is BrowserDialog.Delete -> {
            val failed = stringResource(R.string.delete_failed)
            InkDialog(
                title = stringResource(R.string.delete_question),
                onDismiss = { dialog = null },
                buttons = {
                    InkButton(stringResource(R.string.cancel), onClick = { dialog = null }, modifier = Modifier.weight(1f))
                    InkButton(stringResource(R.string.delete), primary = true, modifier = Modifier.weight(1f), onClick = {
                        dialog = null
                        runOp(failed) { SaveFolder.delete(context, d.entry.uri) }
                    })
                }
            ) {
                Text(
                    if (d.entry.isFolder) stringResource(R.string.delete_folder_text, d.entry.name)
                    else stringResource(R.string.delete_file_text, d.entry.displayName),
                    fontSize = 16.sp
                )
            }
        }
        is BrowserDialog.Move -> {
            val failed = stringResource(R.string.move_failed)
            MoveDialog(
                context = context,
                tree = tree,
                entry = d.entry,
                startPath = path,
                onDismiss = { dialog = null },
                onMove = { targetId ->
                    dialog = null
                    runOp(failed) { SaveFolder.move(context, tree, d.entry, current.id, targetId) }
                }
            )
        }
    }
}

// ── Pieces ───────────────────────────────────────────────────────────────────

@Composable
private fun FilesHeader(title: String, count: Int) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
        Text(title, fontSize = 24.sp, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
        Text(stringResource(R.string.file_count, count), fontSize = 16.sp)
    }
}

@Composable
private fun EmptyNote(text: String) {
    Text(
        text,
        fontSize = 16.sp,
        textAlign = TextAlign.Center,
        modifier = Modifier.fillMaxWidth().padding(vertical = 28.dp)
    )
}

private fun <T> Set<T>.toggle(item: T): Set<T> = if (item in this) this - item else this + item

/**
 * Above the list: "Select" and "Clear", or while selecting, select all/none,
 * delete the selection and leave. [onClear] null hides "Clear".
 */
@Composable
private fun SelectionBar(
    selecting: Boolean,
    selected: Int,
    all: Boolean,
    onSelect: () -> Unit,
    onClear: (() -> Unit)?,
    onToggleAll: () -> Unit,
    onDelete: () -> Unit,
    onDone: () -> Unit,
) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        if (selecting) {
            InkButton(stringResource(if (all) R.string.select_none else R.string.select_all), onClick = onToggleAll, height = 48.dp, modifier = Modifier.weight(1f))
            InkButton(stringResource(R.string.delete_count, selected), onClick = onDelete, enabled = selected > 0, primary = true, height = 48.dp, modifier = Modifier.weight(1f))
            InkButton(stringResource(R.string.done), onClick = onDone, height = 48.dp, modifier = Modifier.weight(1f))
        } else {
            InkButton(stringResource(R.string.select), onClick = onSelect, height = 48.dp, modifier = Modifier.weight(1f))
            if (onClear != null) {
                InkButton(stringResource(R.string.clear), onClick = onClear, height = 48.dp, modifier = Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun ConfirmDelete(text: String, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    InkDialog(
        title = stringResource(R.string.delete_question),
        onDismiss = onDismiss,
        buttons = {
            InkButton(stringResource(R.string.cancel), onClick = onDismiss, modifier = Modifier.weight(1f))
            InkButton(stringResource(R.string.delete), primary = true, onClick = onConfirm, modifier = Modifier.weight(1f))
        }
    ) {
        Text(text, fontSize = 16.sp)
    }
}

/** One entry: kind tag, name and details; "⋯" opens its actions. [checked] non-null shows a check box. */
@Composable
private fun EntryRow(
    tag: String,
    name: String,
    subtitle: String,
    onClick: () -> Unit,
    onMore: (() -> Unit)?,
    checked: Boolean? = null,
) {
    Row(
        modifier = Modifier.fillMaxWidth().heightIn(min = 68.dp).border(if (checked == true) 3.dp else 1.5.dp, if (checked == true) Accent else Ink),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Row(
            Modifier.weight(1f).tap(onClick = onClick).padding(10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            if (checked != null) {
                Box(
                    Modifier.size(28.dp).border(2.dp, if (checked) Accent else Ink).background(if (checked) Accent else Paper),
                    contentAlignment = Alignment.Center
                ) {
                    if (checked) Text("✓", color = Paper, fontSize = 18.sp, fontWeight = FontWeight.Bold)
                }
                Spacer(Modifier.width(10.dp))
            }
            InkTag(tag)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(name, fontSize = 16.sp, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(subtitle, fontSize = 14.sp)
            }
        }
        if (onMore != null) {
            Box(
                Modifier.width(56.dp).heightIn(min = 68.dp).tap(onClick = onMore),
                contentAlignment = Alignment.Center
            ) {
                Text("⋯", fontSize = 26.sp, fontWeight = FontWeight.Bold)
            }
        }
    }
}

/** Previous / next with the page number between: large targets, no scrolling. */
@Composable
private fun Pager(page: Int, pages: Int, onPage: (Int) -> Unit) {
    if (pages <= 1) return
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        InkButton(stringResource(R.string.prev_page), onClick = { onPage(page - 1) }, enabled = page > 0, modifier = Modifier.weight(1f))
        Text("${page + 1} / $pages", fontSize = 18.sp, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center, modifier = Modifier.width(90.dp))
        InkButton(stringResource(R.string.next_page), onClick = { onPage(page + 1) }, enabled = page < pages - 1, modifier = Modifier.weight(1f))
    }
}

@Composable
private fun NameDialog(
    title: String,
    initial: String,
    confirmLabel: String,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    // Preselect the name without its extension, so typing replaces just that
    var field by remember {
        val stem = initial.substringBeforeLast('.').length.takeIf { '.' in initial } ?: initial.length
        mutableStateOf(TextFieldValue(initial, TextRange(0, stem)))
    }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val trimmed = field.text.trim()
    // '/' would create a path on most providers; leading dots hide the entry
    val valid = trimmed.isNotEmpty() && '/' !in trimmed && !trimmed.startsWith(".") && trimmed != initial
    InkDialog(
        title = title,
        onDismiss = onDismiss,
        buttons = {
            InkButton(stringResource(R.string.cancel), onClick = onDismiss, modifier = Modifier.weight(1f))
            InkButton(confirmLabel, onClick = { onConfirm(trimmed) }, enabled = valid, primary = true, modifier = Modifier.weight(1f))
        }
    ) {
        OutlinedTextField(
            value = field,
            onValueChange = { field = it },
            singleLine = true,
            textStyle = TextStyle(fontSize = 17.sp),
            modifier = Modifier.fillMaxWidth().focusRequester(focus)
        )
    }
}

/** Pick a destination folder inside the save folder, starting from where the entry is. */
@Composable
private fun MoveDialog(
    context: Context,
    tree: Uri,
    entry: DocEntry,
    startPath: List<Crumb>,
    onDismiss: () -> Unit,
    onMove: (String) -> Unit,
) {
    val fromId = startPath.last().id
    var path by remember { mutableStateOf(startPath) }
    val here = path.last()
    val folders by produceState<List<DocEntry>>(emptyList(), here) {
        value = withContext(Dispatchers.IO) {
            runCatching { SaveFolder.list(context, tree, here.id) }.getOrDefault(emptyList())
                // A folder can't be moved into itself
                .filter { it.isFolder && it.documentId != entry.documentId }
        }
    }
    InkDialog(
        title = stringResource(R.string.move_title, entry.displayName),
        onDismiss = onDismiss,
        buttons = {
            InkButton(stringResource(R.string.cancel), onClick = onDismiss, modifier = Modifier.weight(1f))
            InkButton(stringResource(R.string.move_here), onClick = { onMove(here.id) }, enabled = here.id != fromId, primary = true, modifier = Modifier.weight(1f))
        }
    ) {
        Text(path.joinToString(" / ") { it.name }, fontSize = 15.sp, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis)
        if (path.size > 1) {
            InkButton(stringResource(R.string.up), onClick = { path = path.dropLast(1) }, height = 48.dp, modifier = Modifier.fillMaxWidth())
        }
        if (folders.isEmpty()) {
            Text(stringResource(R.string.no_subfolders), fontSize = 15.sp)
        }
        folders.forEach { folder ->
            EntryRow(
                tag = stringResource(R.string.tag_folder),
                name = folder.name,
                subtitle = stringResource(R.string.folder),
                onClick = { path = path + Crumb(folder.documentId, folder.name) },
                onMore = null
            )
        }
    }
}
