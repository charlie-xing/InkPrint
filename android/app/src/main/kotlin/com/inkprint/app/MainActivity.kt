package com.inkprint.app

import android.Manifest
import android.content.*
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.enableEdgeToEdge
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.animation.AnimatedVisibility
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.text.SimpleDateFormat
import java.util.*

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
                Toast.makeText(this, "Cannot use this folder: ${e.message}", Toast.LENGTH_LONG).show()
                return@registerForActivityResult
            }
            lifecycleScope.launch {
                withContext(Dispatchers.IO) { SaveFolder.adoptStagedJobs(this@MainActivity, uri) }
                treeUri = uri
                jobTick++
            }
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
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    InkPrintScreen()
                }
            }
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
            Toast.makeText(this, "No app can open this file", Toast.LENGTH_SHORT).show()
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
            Toast.makeText(this, "This device has no system folder picker", Toast.LENGTH_LONG).show()
        }
    }

    // ── Main screen ──────────────────────────────────────────────────────────

    @Composable
    fun InkPrintScreen() {
        var isRunning by remember { mutableStateOf(false) }
        val port          = PrinterService.DEFAULT_PORT.toInt()
        // Re-read on resume: Wi-Fi or Tailscale may have come up or changed address
        val addresses     = remember(jobTick) { PrinterAddresses.list() }
        val onWifi        = addresses.any { it.label == "Wi-Fi" || it.label == "Ethernet" }

        val files = remember(jobTick) { getStoredFiles() }

        Column(
            modifier = Modifier
                .fillMaxSize()
                .safeDrawingPadding()
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Text("InkPrint", fontSize = 28.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.height(2.dp))
            Text("IPP Virtual Printer", color = Color.Gray, fontSize = 14.sp)
            Spacer(Modifier.height(14.dp))

            // Network warning: nothing reachable, or reachable over VPN only
            if (!onWifi) {
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF3E0))
                ) {
                    Row(modifier = Modifier.padding(12.dp), verticalAlignment = Alignment.Top) {
                        Text("⚠️", fontSize = 16.sp)
                        Spacer(Modifier.width(8.dp))
                        Text(
                            if (addresses.isEmpty())
                                "WiFi not connected — devices on the LAN cannot reach this printer. Please connect to WiFi (or a VPN such as Tailscale) first."
                            else
                                "WiFi not connected — only devices on ${addresses.joinToString(" / ") { it.label }} can reach this printer, and auto-discovery is unavailable.",
                            fontSize = 13.sp, color = Color(0xFFBF360C)
                        )
                    }
                }
                Spacer(Modifier.height(12.dp))
            }

            // Start / Stop
            Button(
                onClick = {
                    if (isRunning) { stopPrinterService(); isRunning = false }
                    else { startPrinterService(); isRunning = true }
                },
                colors = ButtonDefaults.buttonColors(
                    containerColor = if (isRunning) Color(0xFFD32F2F) else Color(0xFF388E3C)
                ),
                modifier = Modifier.fillMaxWidth().height(56.dp)
            ) {
                Text(if (isRunning) "Stop Printer Service" else "Start Printer Service", fontSize = 17.sp)
            }

            Spacer(Modifier.height(12.dp))

            // Status
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row {
                        Text("Status: ", fontWeight = FontWeight.Medium)
                        Text(
                            if (isRunning) "Running" else "Stopped",
                            color = if (isRunning) Color(0xFF388E3C) else Color.Gray
                        )
                    }
                    if (isRunning) {
                        addresses.forEach { addr ->
                            Spacer(Modifier.height(4.dp))
                            Text(addr.label, fontSize = 12.sp, color = Color.Gray)
                            CopyableText(addr.printerUrl(port))
                        }
                    }
                }
            }

            Spacer(Modifier.height(16.dp))

            // File browser
            val tree = treeUri
            if (tree != null) {
                FolderBrowserCard(
                    tree = tree,
                    refreshTick = jobTick,
                    onOpen = { openUri(it.uri, it.mimeType) },
                    onShare = { shareUri(it.uri, it.mimeType) },
                    onChangeFolder = { pickSaveFolder() }
                )
            } else {
                FileBrowserCard(
                    files = files,
                    onOpen = { openFile(it) },
                    onChooseFolder = { pickSaveFolder() }
                )
            }

            Spacer(Modifier.height(12.dp))

            // Collapsible help
            AddPrinterInstructionsCard(addresses = addresses, port = port.toString())

            Spacer(Modifier.height(16.dp))

            // Exit + Feedback
            var showFeedbackDialog by remember { mutableStateOf(false) }

            if (showFeedbackDialog) {
                AlertDialog(
                    onDismissRequest = { showFeedbackDialog = false },
                    title = { Text("Feedback") },
                    text = {
                        Text(
                            "InkPrint is an open-source project.\n\n" +
                            "If you have any suggestions or encounter issues, feel free to send an email to:\n\n" +
                            "xjohn1666@gmail.com\n\n" +
                            "The author will get back to you as soon as possible. Thank you for your support!"
                        )
                    },
                    confirmButton = {
                        TextButton(onClick = { showFeedbackDialog = false }) {
                            Text("OK")
                        }
                    }
                )
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                OutlinedButton(
                    onClick = { exitApp() },
                    modifier = Modifier.weight(1f),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = Color(0xFFB71C1C))
                ) {
                    Text("Exit App", fontSize = 15.sp)
                }
                OutlinedButton(
                    onClick = { showFeedbackDialog = true },
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Feedback", fontSize = 15.sp)
                }
            }

            Spacer(Modifier.height(16.dp))
        }
    }
}

data class FileEntry(val name: String, val path: String, val sizeBytes: Long, val modifiedMs: Long) {
    /** "1790148775_4_Weekly_Notes.pdf" -> "Weekly Notes.pdf": drops the core's timestamp/job-id prefix. */
    val displayName: String
        get() = name.replace(Regex("^\\d+_\\d+_"), "").replace('_', ' ').ifBlank { name }
}

// ── File browser card ────────────────────────────────────────────────────────

internal const val PAGE_SIZE = 5

@Composable
fun FileBrowserCard(files: List<FileEntry>, onOpen: (String) -> Unit, onChooseFolder: () -> Unit) {
    val dateFmt = remember { SimpleDateFormat("MM/dd HH:mm", Locale.getDefault()) }
    var currentPage by remember { mutableStateOf(0) }

    // Reset to first page when file list changes
    val totalPages = maxOf(1, (files.size + PAGE_SIZE - 1) / PAGE_SIZE)
    if (currentPage >= totalPages) currentPage = 0

    val pageFiles = files.drop(currentPage * PAGE_SIZE).take(PAGE_SIZE)

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
                Text("${files.size}", color = Color.Gray, fontSize = 13.sp)
            }

            Spacer(Modifier.height(6.dp))
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(6.dp))
                    .background(Color(0xFFE8F0FE))
                    .clickable { onChooseFolder() }
                    .padding(horizontal = 10.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    "Choose a save folder to organize files into folders, rename, move and delete them",
                    fontSize = 12.sp,
                    color = Color(0xFF1565C0),
                    modifier = Modifier.weight(1f)
                )
                Spacer(Modifier.width(8.dp))
                Text("Choose", fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = Color(0xFF1565C0))
            }
            Spacer(Modifier.height(8.dp))

            if (files.isEmpty()) {
                Box(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 20.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text("No files yet — print something!", color = Color.Gray, fontSize = 13.sp)
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    pageFiles.forEach { file ->
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(6.dp))
                                .background(Color(0xFFF5F5F5))
                                .clickable { onOpen(file.path) }
                                .padding(horizontal = 10.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(fileIcon(file.name), fontSize = 20.sp)
                            Spacer(Modifier.width(10.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    file.displayName,
                                    fontSize = 13.sp,
                                    fontWeight = FontWeight.Medium,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis
                                )
                                Text(
                                    "${dateFmt.format(Date(file.modifiedMs))}  ${formatSize(file.sizeBytes)}",
                                    fontSize = 11.sp,
                                    color = Color.Gray
                                )
                            }
                        }
                    }
                }

                // Pagination controls (only when more than one page)
                if (totalPages > 1) {
                    Spacer(Modifier.height(10.dp))
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        TextButton(
                            onClick = { if (currentPage > 0) currentPage-- },
                            enabled = currentPage > 0
                        ) {
                            Text("◀  Prev", fontSize = 13.sp)
                        }
                        Text(
                            "${currentPage + 1} / $totalPages",
                            fontSize = 13.sp,
                            color = Color.Gray,
                            textAlign = TextAlign.Center
                        )
                        TextButton(
                            onClick = { if (currentPage < totalPages - 1) currentPage++ },
                            enabled = currentPage < totalPages - 1
                        ) {
                            Text("Next  ▶", fontSize = 13.sp)
                        }
                    }
                }
            }
        }
    }
}

internal fun fileIcon(name: String) = when (name.substringAfterLast('.').lowercase()) {
    "pdf" -> "\uD83D\uDCC4"
    "ps"  -> "\uD83D\uDDA8"
    else  -> "\uD83D\uDCC1"
}

internal fun formatSize(bytes: Long): String = when {
    bytes < 1024            -> "$bytes B"
    bytes < 1024 * 1024     -> "${bytes / 1024} KB"
    else                    -> "${bytes / (1024 * 1024)} MB"
}

// ── Collapsible instructions card ────────────────────────────────────────────

@Composable
fun AddPrinterInstructionsCard(addresses: List<PrinterAddress>, port: String) {
    var expanded by remember { mutableStateOf(false) }
    // Which network the steps are written for; defaults to the first (Wi-Fi when present)
    var selectedLabel by remember { mutableStateOf<String?>(null) }
    val selected = addresses.firstOrNull { it.label == selectedLabel } ?: addresses.firstOrNull()
    val ip = selected?.ip ?: "<device IP>"

    Card(modifier = Modifier.fillMaxWidth()) {
        Column {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { expanded = !expanded }
                    .padding(14.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    "How to add this printer",
                    fontWeight = FontWeight.Bold,
                    fontSize = 15.sp,
                    modifier = Modifier.weight(1f)
                )
                Text(if (expanded) "▲" else "▼", color = Color.Gray, fontSize = 12.sp)
            }

            AnimatedVisibility(visible = expanded) {
                Column(
                    modifier = Modifier.padding(start = 14.dp, end = 14.dp, bottom = 14.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    Text(
                        "Auto-discovery only works on the same WiFi. Over Tailscale / VPN, use the manual steps with this device's VPN IP. " +
                        "Always choose the driverless option (AirPrint, IPP Everywhere, IPP Class Driver) — InkPrint only accepts PDF.",
                        fontSize = 12.sp, color = Color.Gray
                    )
                    if (addresses.size > 1) {
                        Text("Show the steps for:", fontSize = 12.sp, fontWeight = FontWeight.Medium)
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            addresses.forEach { addr ->
                                FilterChip(
                                    selected = addr == selected,
                                    onClick = { selectedLabel = addr.label },
                                    label = { Text("${addr.label}  ${addr.ip}", fontSize = 12.sp) }
                                )
                            }
                        }
                    }
                    OsSection("macOS")   { MacOsInstructions(ip, port) }
                    OsSection("Windows") { WindowsInstructions(ip, port) }
                    OsSection("Linux")   { LinuxInstructions(ip, port) }
                    OsSection("iOS / iPadOS") { IosInstructions() }
                    OsSection("Android") { AndroidInstructions(ip, port) }
                }
            }
        }
    }
}

@Composable
fun OsSection(title: String, content: @Composable () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(6.dp))
                .background(Color(0xFFF3F4F6))
                .clickable { open = !open }
                .padding(horizontal = 12.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                title,
                fontWeight = FontWeight.SemiBold,
                fontSize = 14.sp,
                modifier = Modifier.weight(1f)
            )
            Text(if (open) "▲" else "▼", color = Color.Gray, fontSize = 11.sp)
        }
        AnimatedVisibility(visible = open) {
            Column(modifier = Modifier.padding(top = 10.dp, bottom = 4.dp)) {
                content()
            }
        }
        Spacer(Modifier.height(4.dp))
    }
}

// ── OS instruction panels ────────────────────────────────────────────────────

@Composable
fun MacOsInstructions(ip: String, port: String) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {

        InfoBadge("✅ Recommended — Auto-discovery (AirPrint)")
        Text(
            "Make sure InkPrint service is running and your Mac is on the same WiFi network.",
            fontSize = 12.sp, color = Color.Gray
        )
        Step(1, "Apple menu → System Settings → Printers & Scanners")
        Step(2, "Click Add Printer, Scanner or Fax…")
        Step(3, "InkPrint appears in the list — select it")
        Step(4, "Use: AirPrint is selected automatically → click Add")

        HorizontalDivider()

        InfoBadge("🖥️ Manual — Terminal (by IP, e.g. over Tailscale/VPN)")
        CodeBlock(
            "lpadmin -p InkPrint -E \\\n" +
            "  -v ipp://$ip:$port/ipp/print \\\n" +
            "  -m everywhere"
        )
        Text(
            "Don't add it with \"Generic PostScript Printer\": that sends PostScript, and InkPrint only accepts PDF.",
            fontSize = 12.sp, color = Color(0xFFBF360C)
        )
    }
}

@Composable
fun WindowsInstructions(ip: String, port: String) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {

        InfoBadge("✅ Automatic")
        Step(1, "Settings → Bluetooth & devices → Printers & scanners")
        Step(2, "Click Add device — InkPrint appears on the same network")
        Step(3, "Click Add device to confirm")
        Text(
            "If InkPrint doesn't show up, add it by IP address below.",
            fontSize = 12.sp, color = Color.Gray
        )

        HorizontalDivider()

        InfoBadge("🔧 Manual — Add by IP address")
        Step(1, "Settings → Bluetooth & devices → Printers & scanners")
        Step(2, "Add device → \"The printer that I want isn't listed\"")
        Step(3, "Select \"Add a printer using an IP address or hostname\"")
        Step(4, "Protocol: IPP  /  Hostname or IP address:")
        CodeBlock("$ip")
        Step(5, "Port number: $port  /  Queue: ipp/print")
        Step(6, "Driver: Microsoft IPP Class Driver — then click Next to finish")
        Text(
            "Don't pick Generic / Text Only or a PostScript driver: they don't send PDF, and the job is rejected.",
            fontSize = 12.sp, color = Color(0xFFBF360C)
        )
    }
}

@Composable
fun LinuxInstructions(ip: String, port: String) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {

        InfoBadge("✅ CUPS — IPP Everywhere (recommended)")
        Text("Works on Ubuntu, Debian, Fedora, Arch, and most distros.", fontSize = 12.sp, color = Color.Gray)
        CodeBlock(
            "# Add printer (driverless IPP Everywhere)\n" +
            "sudo lpadmin -p InkPrint -E \\\n" +
            "  -v ipp://$ip:$port/ipp/print \\\n" +
            "  -m everywhere\n\n" +
            "# Set as default (optional)\n" +
            "sudo lpoptions -d InkPrint\n\n" +
            "# Print a file\n" +
            "lp -d InkPrint /path/to/document.pdf"
        )

        HorizontalDivider()

        InfoBadge("🖥️ GNOME / KDE GUI")
        Step(1, "Settings → Printers → Add a Printer")
        Step(2, "Enter the IPP address manually:")
        CodeBlock("ipp://$ip:$port/ipp/print")
        Step(3, "Driver: IPP Everywhere (driverless) → Apply")
        Text(
            "Don't pick a Generic, PostScript or vendor driver: those don't send PDF.",
            fontSize = 12.sp, color = Color(0xFFBF360C)
        )
    }
}

@Composable
fun IosInstructions() {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {

        InfoBadge("✅ AirPrint — Zero configuration")
        Text(
            "iPhone and iPad support AirPrint natively. No app or setup needed.",
            fontSize = 12.sp, color = Color.Gray
        )
        Step(1, "Start InkPrint service on your BOOX device")
        Step(2, "Connect your iPhone/iPad to the same WiFi network")
        Step(3, "Open any app (Safari, Files, Mail, Photos…)")
        Step(4, "Tap the Share button  →  Print")
        Step(5, "Tap Select Printer — InkPrint appears automatically")
        Step(6, "Tap Print — the file is saved on the BOOX as PDF")

        HorizontalDivider()

        InfoBadge("⚠️ Hotspot limitation")
        Text(
            "If your iPhone is sharing a Personal Hotspot, the BOOX device connected to it cannot be discovered via AirPrint. Use a shared WiFi router instead.",
            fontSize = 12.sp, color = Color(0xFFBF360C)
        )
    }
}

@Composable
fun AndroidInstructions(ip: String, port: String) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {

        InfoBadge("✅ Built-in Android Print Service")
        Text("Android 8+ includes an IPP-capable print service.", fontSize = 12.sp, color = Color.Gray)
        Step(1, "Start InkPrint service on BOOX and connect to the same WiFi")
        Step(2, "Settings → Connected devices → Connection preferences → Printing")
        Step(3, "Tap Default Print Service → Enable it")
        Step(4, "InkPrint should appear automatically")
        Step(5, "In any app, open Share / Print menu → select InkPrint")

        HorizontalDivider()

        InfoBadge("🖨️ Manual — Add by IP address")
        Text("If auto-discovery doesn't find InkPrint:", fontSize = 12.sp, color = Color.Gray)
        Step(1, "In Default Print Service, tap ⋮ → Add printer")
        Step(2, "Add printer by IP address, and enter (with the port):")
        CodeBlock("$ip:$port")
    }
}

// ── Shared UI helpers ─────────────────────────────────────────────────────────

@Composable
fun InfoBadge(text: String) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(4.dp))
            .background(Color(0xFFE8F0FE))
            .padding(horizontal = 10.dp, vertical = 6.dp)
    ) {
        Text(text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = Color(0xFF1565C0))
    }
}

@Composable
fun CopyableText(text: String) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(4.dp))
            .background(Color(0xFFF3F4F6))
            .clickable { clipboard.setText(AnnotatedString(text)); copied = true }
            .padding(horizontal = 8.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            text,
            fontFamily = FontFamily.Monospace,
            fontSize = 12.sp,
            color = Color(0xFF1565C0),
            modifier = Modifier.weight(1f)
        )
        Text(if (copied) "Copied!" else "Copy", fontSize = 11.sp, color = Color.Gray)
    }
}

@Composable
fun Step(number: Int, text: String) {
    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
        Box(
            modifier = Modifier
                .size(22.dp)
                .clip(RoundedCornerShape(11.dp))
                .background(Color(0xFF1565C0)),
            contentAlignment = Alignment.Center
        ) {
            Text(number.toString(), color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.Bold)
        }
        Spacer(Modifier.width(8.dp))
        Text(text, fontSize = 13.sp, modifier = Modifier.weight(1f))
    }
}

@Composable
fun CodeBlock(text: String) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(6.dp))
            .background(Color(0xFF1E1E1E))
            .clickable { clipboard.setText(AnnotatedString(text)); copied = true }
            .padding(10.dp)
    ) {
        Column {
            Text(text, fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = Color(0xFFD4D4D4))
            Spacer(Modifier.height(4.dp))
            Text(if (copied) "Copied!" else "Tap to copy", fontSize = 10.sp, color = Color(0xFF888888))
        }
    }
}
