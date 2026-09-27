package com.inkprint.app

import android.content.Context
import android.net.Uri
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextRange
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
    data class Rename(val entry: DocEntry) : BrowserDialog
    data class Move(val entry: DocEntry) : BrowserDialog
    data class Delete(val entry: DocEntry) : BrowserDialog
}

// ── Save-folder browser card ─────────────────────────────────────────────────

/**
 * Browses and manages the user's save folder: walk into subfolders, create
 * folders, and open, share, rename, move or delete entries.
 */
@Composable
fun FolderBrowserCard(
    tree: Uri,
    refreshTick: Int,
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
    var currentPage by remember(tree, current) { mutableStateOf(0) }
    var dialog by remember { mutableStateOf<BrowserDialog?>(null) }

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

    val totalPages = maxOf(1, (entries.size + PAGE_SIZE - 1) / PAGE_SIZE)
    if (currentPage >= totalPages) currentPage = 0
    val pageEntries = entries.drop(currentPage * PAGE_SIZE).take(PAGE_SIZE)

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    "Printed Files",
                    fontWeight = FontWeight.Bold,
                    fontSize = 15.sp,
                    modifier = Modifier.weight(1f)
                )
                Text("${entries.size}", color = Color.Gray, fontSize = 13.sp)
            }

            // Breadcrumb: tap a segment to jump back to it
            Spacer(Modifier.height(6.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("📂", fontSize = 14.sp)
                Spacer(Modifier.width(4.dp))
                Row(modifier = Modifier.weight(1f)) {
                    path.forEachIndexed { i, crumb ->
                        if (i > 0) Text(" › ", fontSize = 12.sp, color = Color.Gray)
                        val last = i == path.lastIndex
                        Text(
                            crumb.name,
                            fontSize = 12.sp,
                            color = if (last) Color.Unspecified else Color(0xFF1565C0),
                            fontWeight = if (last) FontWeight.SemiBold else FontWeight.Normal,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier
                                .weight(1f, fill = false)
                                .clickable(enabled = !last) { path = path.take(i + 1) }
                        )
                    }
                }
            }

            // Toolbar
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                if (path.size > 1) {
                    TextButton(onClick = { path = path.dropLast(1) }) { Text("⬆  Up", fontSize = 13.sp) }
                }
                TextButton(onClick = { dialog = BrowserDialog.NewFolder }) { Text("+ New folder", fontSize = 13.sp) }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = onChangeFolder) { Text("Change folder", fontSize = 13.sp) }
            }

            when {
                listing == null -> EmptyNote("Loading…")
                listing!!.isFailure -> EmptyNote("Cannot read this folder — choose the save folder again.")
                entries.isEmpty() -> EmptyNote(
                    if (path.size == 1) "No files yet — print something!" else "This folder is empty"
                )
                else -> Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    pageEntries.forEach { entry ->
                        EntryRow(
                            entry = entry,
                            subtitle = if (entry.isFolder) "Folder"
                                else "${dateFmt.format(Date(entry.modifiedMs))}  ${formatSize(entry.sizeBytes)}",
                            onClick = {
                                if (entry.isFolder) path = path + Crumb(entry.documentId, entry.name)
                                else onOpen(entry)
                            },
                            onShare = { onShare(entry) },
                            onRename = { dialog = BrowserDialog.Rename(entry) },
                            onMove = { dialog = BrowserDialog.Move(entry) },
                            onDelete = { dialog = BrowserDialog.Delete(entry) },
                        )
                    }
                }
            }

            if (totalPages > 1) {
                Spacer(Modifier.height(10.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    TextButton(onClick = { currentPage-- }, enabled = currentPage > 0) {
                        Text("◀  Prev", fontSize = 13.sp)
                    }
                    Text(
                        "${currentPage + 1} / $totalPages",
                        fontSize = 13.sp,
                        color = Color.Gray,
                        textAlign = TextAlign.Center
                    )
                    TextButton(onClick = { currentPage++ }, enabled = currentPage < totalPages - 1) {
                        Text("Next  ▶", fontSize = 13.sp)
                    }
                }
            }
        }
    }

    when (val d = dialog) {
        null -> {}
        BrowserDialog.NewFolder -> NameDialog(
            title = "New folder",
            initial = "",
            confirmLabel = "Create",
            onDismiss = { dialog = null },
            onConfirm = { name ->
                dialog = null
                runOp("Could not create folder") { SaveFolder.createFolder(context, tree, current.id, name) }
            }
        )
        is BrowserDialog.Rename -> NameDialog(
            title = "Rename",
            initial = d.entry.name,
            confirmLabel = "Rename",
            onDismiss = { dialog = null },
            onConfirm = { name ->
                dialog = null
                runOp("Could not rename") { SaveFolder.rename(context, d.entry.uri, name) }
            }
        )
        is BrowserDialog.Delete -> AlertDialog(
            onDismissRequest = { dialog = null },
            title = { Text("Delete?") },
            text = {
                Text(
                    if (d.entry.isFolder) "\"${d.entry.name}\" and everything in it will be deleted."
                    else "\"${d.entry.displayName}\" will be deleted."
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    dialog = null
                    runOp("Could not delete") { SaveFolder.delete(context, d.entry.uri) }
                }) { Text("Delete", color = Color(0xFFD32F2F)) }
            },
            dismissButton = { TextButton(onClick = { dialog = null }) { Text("Cancel") } }
        )
        is BrowserDialog.Move -> MoveDialog(
            context = context,
            tree = tree,
            entry = d.entry,
            startPath = path,
            onDismiss = { dialog = null },
            onMove = { targetId ->
                dialog = null
                runOp("Could not move") { SaveFolder.move(context, tree, d.entry, current.id, targetId) }
            }
        )
    }
}

@Composable
private fun EmptyNote(text: String) {
    Box(
        modifier = Modifier.fillMaxWidth().padding(vertical = 20.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(text, color = Color.Gray, fontSize = 13.sp, textAlign = TextAlign.Center)
    }
}

@Composable
private fun EntryRow(
    entry: DocEntry,
    subtitle: String,
    onClick: () -> Unit,
    onShare: () -> Unit,
    onRename: () -> Unit,
    onMove: () -> Unit,
    onDelete: () -> Unit,
) {
    var menuOpen by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(6.dp))
            .background(Color(0xFFF5F5F5))
            .clickable(onClick = onClick)
            .padding(start = 10.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(if (entry.isFolder) "📁" else fileIcon(entry.name), fontSize = 20.sp)
        Spacer(Modifier.width(10.dp))
        Column(modifier = Modifier.weight(1f).padding(vertical = 4.dp)) {
            Text(
                entry.displayName,
                fontSize = 13.sp,
                fontWeight = FontWeight.Medium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis
            )
            Text(subtitle, fontSize = 11.sp, color = Color.Gray)
        }
        Box {
            TextButton(onClick = { menuOpen = true }) { Text("⋮", fontSize = 18.sp, color = Color.Gray) }
            DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                if (!entry.isFolder) {
                    DropdownMenuItem(text = { Text("Share") }, onClick = { menuOpen = false; onShare() })
                }
                DropdownMenuItem(text = { Text("Rename") }, onClick = { menuOpen = false; onRename() })
                DropdownMenuItem(text = { Text("Move to…") }, onClick = { menuOpen = false; onMove() })
                DropdownMenuItem(
                    text = { Text("Delete", color = Color(0xFFD32F2F)) },
                    onClick = { menuOpen = false; onDelete() }
                )
            }
        }
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
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = field,
                onValueChange = { field = it },
                singleLine = true,
                modifier = Modifier.focusRequester(focus)
            )
        },
        confirmButton = {
            TextButton(onClick = { onConfirm(trimmed) }, enabled = valid) { Text(confirmLabel) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } }
    )
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
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Move \"${entry.displayName}\"") },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(4.dp)
            ) {
                Text(
                    path.joinToString(" › ") { it.name },
                    fontSize = 12.sp,
                    color = Color.Gray,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis
                )
                if (path.size > 1) {
                    TextButton(onClick = { path = path.dropLast(1) }) { Text("⬆  Up") }
                }
                if (folders.isEmpty()) {
                    Text("No subfolders", fontSize = 13.sp, color = Color.Gray)
                }
                folders.forEach { folder ->
                    Text(
                        "📁  ${folder.name}",
                        fontSize = 14.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(4.dp))
                            .clickable { path = path + Crumb(folder.documentId, folder.name) }
                            .padding(vertical = 8.dp, horizontal = 4.dp)
                    )
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { onMove(here.id) }, enabled = here.id != fromId) { Text("Move here") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } }
    )
}
