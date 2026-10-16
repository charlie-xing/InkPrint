package com.inkprint.app

import android.Manifest
import android.content.*
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

class MainActivity : ComponentActivity() {

    // Tick incremented on each print job and on resume — triggers file list refresh
    private var jobTick by mutableStateOf(0)

    private val jobReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == PrinterService.BROADCAST_JOB_RECEIVED) {
                jobTick++
            }
        }
    }

    // Save folder picked by the user; null until one is chosen
    private var treeUri by mutableStateOf<Uri?>(null)

    private val permissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { }

    private val folderPicker =
        registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
            if (uri == null) return@registerForActivityResult
            try {
                SaveFolder.setTree(this, uri)
            } catch (e: SecurityException) {
                Toast.makeText(this, getString(R.string.folder_unusable, e.message), Toast.LENGTH_LONG).show()
                return@registerForActivityResult
            }
            lifecycleScope.launch {
                withContext(Dispatchers.IO) { SaveFolder.adoptStagedJobs(this@MainActivity, uri) }
                treeUri = uri
                jobTick++
            }
        }

    override fun attachBaseContext(newBase: Context) {
        super.attachBaseContext(Lang.wrap(newBase))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        requestRuntimePermissions()
        treeUri = SaveFolder.treeUri(this)
        registerReceiver(
            jobReceiver,
            IntentFilter(PrinterService.BROADCAST_JOB_RECEIVED),
            RECEIVER_NOT_EXPORTED
        )
        setContent {
            InkTheme { InkPrintApp() }
        }
    }

    /** Files may have changed in another app (e.g. deleted in the reader) while we were away. */
    override fun onResume() {
        super.onResume()
        treeUri = SaveFolder.treeUri(this)
        jobTick++
    }

    override fun onDestroy() {
        unregisterReceiver(jobReceiver)
        super.onDestroy()
    }

    // ── Service control ──────────────────────────────────────────────────────

    private fun startPrinterService() =
        startForegroundService(Intent(this, PrinterService::class.java))

    private fun stopPrinterService() =
        startService(Intent(this, PrinterService::class.java).apply { action = PrinterService.ACTION_STOP })

    /** Picks up a changed EPUB printer setting by restarting the server. */
    private fun restartPrinterService() =
        startService(Intent(this, PrinterService::class.java).apply { action = PrinterService.ACTION_RESTART })

    private fun exitApp() {
        stopPrinterService()
        finishAndRemoveTask()
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    /**
     * Without POST_NOTIFICATIONS the foreground service notification — and with
     * it the "document received" alerts — are silently dropped on Android 13+.
     * The legacy write permission is only meaningful on API 28 and below, where
     * publishing to shared Documents/ still goes through the filesystem.
     */
    private fun requestRuntimePermissions() {
        val wanted = buildList {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                add(Manifest.permission.POST_NOTIFICATIONS)
            }
            if (Build.VERSION.SDK_INT <= Build.VERSION_CODES.P) {
                @Suppress("DEPRECATION")
                add(Manifest.permission.WRITE_EXTERNAL_STORAGE)
            }
        }.filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }
        if (wanted.isNotEmpty()) permissionLauncher.launch(wanted.toTypedArray())
    }

    private fun getStoredFiles(): List<FileEntry> =
        JobStorage.listJobs(this)
            .map { FileEntry(it.name, it.absolutePath, it.length(), it.lastModified()) }

    private fun openFile(filePath: String) {
        val file = File(filePath)
        openUri(
            androidx.core.content.FileProvider.getUriForFile(this, "${packageName}.fileprovider", file),
            JobStorage.mimeTypeOf(file)
        )
    }

    private fun openUri(uri: Uri, mime: String) {
        try {
            startActivity(Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mime)
                flags = Intent.FLAG_GRANT_READ_URI_PERMISSION
            })
        } catch (e: Exception) {
            android.util.Log.e("MainActivity", "Cannot open file: ${e.message}")
            Toast.makeText(this, getString(R.string.no_app_to_open), Toast.LENGTH_SHORT).show()
        }
    }

    private fun shareUri(uri: Uri, mime: String) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = mime
            putExtra(Intent.EXTRA_STREAM, uri)
            flags = Intent.FLAG_GRANT_READ_URI_PERMISSION
        }
        startActivity(Intent.createChooser(send, null))
    }

    private fun pickSaveFolder() {
        try {
            folderPicker.launch(SaveFolder.INITIAL_URI)
        } catch (e: ActivityNotFoundException) {
            Toast.makeText(this, getString(R.string.no_folder_picker), Toast.LENGTH_LONG).show()
        }
    }

    // ── Screens ──────────────────────────────────────────────────────────────

    /**
     * Four tabs, each meant to fit one screen: e-ink scrolls badly, so the
     * app pages instead (scrolling stays as a fallback on small screens).
     */
    @Composable
    fun InkPrintApp() {
        var tab by rememberSaveable { mutableIntStateOf(0) }
        val running by PrinterService.running.collectAsState()
        var epubPrinter by remember { mutableStateOf(Settings.epubPrinter(this@MainActivity)) }
        val port = PrinterService.DEFAULT_PORT.toInt()
        // Re-read on resume: Wi-Fi or Tailscale may have come up or changed address
        val addresses = remember(jobTick) { PrinterAddresses.list() }

        Column(Modifier.fillMaxSize().safeDrawingPadding()) {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                when (tab) {
                    0 -> PrinterTab(running, addresses, port, epubPrinter)
                    1 -> FilesTab()
                    2 -> SettingsTab(epubPrinter, onEpubPrinter = { on ->
                        Settings.setEpubPrinter(this@MainActivity, on)
                        epubPrinter = on
                        if (running) restartPrinterService()
                    })
                    else -> HelpTab(addresses, port.toString(), epubPrinter)
                }
            }
            // Whatever tab is open, a running conversion stays in sight.
            val status by ConversionQueue.status.collectAsState()
            status?.takeIf { tab != 0 }?.let { ProgressStrip(it) }
            InkTabBar(
                tabs = listOf(
                    stringResource(R.string.tab_printer),
                    stringResource(R.string.tab_files),
                    stringResource(R.string.tab_settings),
                    stringResource(R.string.tab_help),
                ),
                selected = tab,
                onSelect = { tab = it }
            )
        }
    }

    @Composable
    private fun TabPage(content: @Composable ColumnScope.() -> Unit) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
            content = content
        )
    }

    @Composable
    private fun PrinterTab(running: Boolean, addresses: List<PrinterAddress>, port: Int, epubPrinter: Boolean) {
        val status by ConversionQueue.status.collectAsState()
        val onWifi = addresses.any { it.label == "Wi-Fi" || it.label == "Ethernet" }
        val identity = remember { Settings.identity(this) }
        TabPage {
            Text("InkPrint", fontSize = 30.sp, fontWeight = FontWeight.Bold)

            // Status: inverted while running, so it reads at a glance.
            Row(
                Modifier
                    .fillMaxWidth()
                    .background(if (running) Ink else Paper)
                    .border(2.dp, Ink)
                    .padding(14.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    stringResource(if (running) R.string.status_running else R.string.status_stopped),
                    color = if (running) Paper else Ink,
                    fontSize = 22.sp,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.weight(1f)
                )
                addresses.firstOrNull()?.let {
                    if (running) Text(it.ip, color = Paper, fontSize = 17.sp, fontWeight = FontWeight.Bold)
                }
            }

            if (!onWifi) {
                InkNotice(
                    if (addresses.isEmpty()) stringResource(R.string.warn_no_network)
                    else stringResource(R.string.warn_vpn_only, addresses.joinToString(" / ") { it.label })
                )
            }

            InkButton(
                text = stringResource(if (running) R.string.stop_service else R.string.start_service),
                primary = !running,
                onClick = { if (running) stopPrinterService() else startPrinterService() },
                modifier = Modifier.fillMaxWidth(),
                height = 64.dp
            )

            if (running) {
                addresses.forEach { addr ->
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(addr.label, fontSize = 15.sp, fontWeight = FontWeight.Bold)
                        Text("${stringResource(R.string.pdf_printer)} · ${identity.pdfName}", fontSize = 14.sp)
                        CopyableText(addr.printerUrl(port))
                        if (epubPrinter) {
                            Text("${stringResource(R.string.epub_printer)} · ${identity.epubName}", fontSize = 14.sp)
                            CopyableText(addr.epubPrinterUrl(port))
                        }
                        if (addr.label == "VPN") {
                            Text(stringResource(R.string.vpn_note), fontSize = 14.sp)
                        }
                    }
                }
            } else {
                Text(stringResource(R.string.start_hint), fontSize = 16.sp)
            }

            status?.let { s -> ConversionStatus(s) }
        }
    }

    /** One line above the tab bar: what is being converted, how far, and Cancel. */
    @Composable
    private fun ProgressStrip(s: ConversionQueue.Status) {
        Column(Modifier.fillMaxWidth()) {
            Box(Modifier.fillMaxWidth().height(2.dp).background(Ink))
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(
                        when (s.kind) {
                            ConversionQueue.Kind.EPUB -> stringResource(R.string.progress_converting, s.name)
                            ConversionQueue.Kind.SEARCHABLE -> stringResource(R.string.progress_recognising, s.name)
                        } + " · " + progressText(s),
                        fontSize = 15.sp,
                        fontWeight = FontWeight.Bold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                    InkProgress(if (s.total > 0) s.done.toFloat() / s.total else 0f, height = 10.dp)
                }
                Spacer(Modifier.width(12.dp))
                InkButton(stringResource(R.string.cancel), onClick = { ConversionQueue.cancelCurrent() }, height = 48.dp)
            }
        }
    }

    @Composable
    private fun progressText(s: ConversionQueue.Status): String =
        if (s.total > 0) stringResource(R.string.progress_page, s.done, s.total)
        else stringResource(R.string.progress_preparing)

    @Composable
    private fun ConversionStatus(s: ConversionQueue.Status) {
        InkCard {
            Text(
                when (s.kind) {
                    ConversionQueue.Kind.EPUB -> stringResource(R.string.progress_converting, s.name)
                    ConversionQueue.Kind.SEARCHABLE -> stringResource(R.string.progress_recognising, s.name)
                },
                fontSize = 17.sp,
                fontWeight = FontWeight.Bold,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis
            )
            InkProgress(if (s.total > 0) s.done.toFloat() / s.total else 0f)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(progressText(s), fontSize = 16.sp)
                    if (s.waiting > 0) Text(stringResource(R.string.progress_waiting, s.waiting), fontSize = 14.sp)
                }
                InkButton(stringResource(R.string.cancel), onClick = { ConversionQueue.cancelCurrent() })
            }
        }
    }

    @Composable
    private fun FilesTab() {
        // Rows per page from the space available, so a page never scrolls.
        BoxWithConstraints(Modifier.fillMaxSize()) {
            val rows = ((maxHeight - 260.dp) / 76.dp).toInt().coerceIn(3, 12)
            TabPage {
                val tree = treeUri
                if (tree != null) {
                    FolderBrowser(
                        tree = tree,
                        refreshTick = jobTick,
                        pageSize = rows,
                        onOpen = { openUri(it.uri, it.mimeType) },
                        onShare = { shareUri(it.uri, it.mimeType) },
                        onChangeFolder = { pickSaveFolder() }
                    )
                } else {
                    val files = remember(jobTick) { getStoredFiles() }
                    FileList(
                        files = files,
                        pageSize = rows,
                        onOpen = { openFile(it) },
                        onChooseFolder = { pickSaveFolder() }
                    )
                }
            }
        }
    }

    @Composable
    private fun SettingsTab(epubPrinter: Boolean, onEpubPrinter: (Boolean) -> Unit) {
        var keepPdf by remember { mutableStateOf(Settings.keepPdf(this)) }
        var ocr by remember { mutableStateOf(Settings.ocr(this)) }
        var searchable by remember { mutableStateOf(Settings.searchablePdf(this)) }
        var dialog by remember { mutableStateOf<String?>(null) }

        TabPage {
            InkHeading(stringResource(R.string.language))
            InkSegmented(
                options = listOf(
                    Lang.Choice.SYSTEM to stringResource(R.string.language_system),
                    Lang.Choice.ENGLISH to "English",
                    Lang.Choice.SIMPLIFIED to "简体中文",
                    Lang.Choice.TRADITIONAL to "繁體中文",
                ),
                selected = Lang.choice(this@MainActivity),
                onSelect = { choice ->
                    if (choice != Lang.choice(this@MainActivity)) {
                        Lang.set(this@MainActivity, choice)
                        recreate()
                    }
                }
            )

            if (BuildConfig.EPUB) {
                InkHeading(stringResource(R.string.epub_printer))
                Text(stringResource(R.string.epub_printer_about), fontSize = 15.sp)
                InkToggle(stringResource(R.string.epub_printer_toggle), stringResource(R.string.epub_printer_toggle_sub), epubPrinter, onEpubPrinter)
                if (epubPrinter) {
                    InkToggle(stringResource(R.string.ocr_toggle), stringResource(R.string.ocr_toggle_sub), ocr) {
                        Settings.setOcr(this@MainActivity, it); ocr = it
                    }
                    InkToggle(stringResource(R.string.keep_pdf_toggle), stringResource(R.string.keep_pdf_toggle_sub), keepPdf) {
                        Settings.setKeepPdf(this@MainActivity, it); keepPdf = it
                    }
                }

                InkHeading(stringResource(R.string.pdf_printer))
                InkToggle(stringResource(R.string.searchable_toggle), stringResource(R.string.searchable_toggle_sub), searchable) {
                    Settings.setSearchablePdf(this@MainActivity, it); searchable = it
                }
            }

            InkHeading(stringResource(R.string.save_folder))
            Text(
                treeUri?.let { SaveFolder.label(it) } ?: stringResource(R.string.save_folder_default),
                fontSize = 16.sp
            )
            InkButton(
                stringResource(if (treeUri == null) R.string.choose_folder else R.string.change_folder),
                onClick = { pickSaveFolder() },
                modifier = Modifier.fillMaxWidth()
            )

            InkHeading(stringResource(R.string.about))
            Text(stringResource(R.string.version, BuildConfig.VERSION_NAME), fontSize = 15.sp)
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                InkButton(stringResource(R.string.feedback), onClick = { dialog = "feedback" }, modifier = Modifier.weight(1f))
                InkButton(stringResource(R.string.licenses), onClick = { dialog = "licenses" }, modifier = Modifier.weight(1f))
            }
            InkButton(stringResource(R.string.exit_app), onClick = { exitApp() }, modifier = Modifier.fillMaxWidth())
        }

        when (dialog) {
            "feedback" -> InkDialog(
                title = stringResource(R.string.feedback),
                onDismiss = { dialog = null },
                buttons = { InkButton(stringResource(R.string.ok), onClick = { dialog = null }, primary = true, modifier = Modifier.fillMaxWidth()) }
            ) {
                Text(stringResource(R.string.feedback_text, "xjohn1666@gmail.com"), fontSize = 16.sp)
            }
            "licenses" -> InkDialog(
                title = stringResource(R.string.licenses),
                onDismiss = { dialog = null },
                buttons = { InkButton(stringResource(R.string.ok), onClick = { dialog = null }, primary = true, modifier = Modifier.fillMaxWidth()) }
            ) {
                Text(
                    stringResource(R.string.licenses_intro) + "\n\n" +
                        (if (BuildConfig.EPUB)
                            "• PP-OCRv6, PP-DocLayout-S — PaddlePaddle, Apache-2.0\n" +
                            "• oar-ocr — Apache-2.0\n" +
                            "• ONNX Runtime — Microsoft, MIT\n" +
                            "• PDFium — BSD-3-Clause / Apache-2.0\n" +
                            "• pdfium-render — MIT / Apache-2.0\n" +
                            "• lopdf — MIT\n\n"
                        else "") +
                        stringResource(R.string.licenses_more),
                    fontSize = 15.sp
                )
            }
        }
    }
}

data class FileEntry(val name: String, val path: String, val sizeBytes: Long, val modifiedMs: Long) {
    /** "1790148775_4_Weekly_Notes.pdf" -> "Weekly Notes.pdf": drops the core's timestamp/job-id prefix. */
    val displayName: String
        get() = name.replace(Regex("^\\d+_\\d+_"), "").replace('_', ' ').ifBlank { name }
}

/** Short label for a file's kind, shown in a frame instead of a colour icon. */
internal fun fileTag(name: String) = when (name.substringAfterLast('.').lowercase()) {
    "pdf" -> "PDF"
    "epub" -> "EPUB"
    else -> name.substringAfterLast('.', "").uppercase().take(4).ifEmpty { "FILE" }
}

internal fun formatSize(bytes: Long): String = when {
    bytes < 1024            -> "$bytes B"
    bytes < 1024 * 1024     -> "${bytes / 1024} KB"
    else                    -> "${bytes / (1024 * 1024)} MB"
}
