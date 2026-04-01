package com.inkprint.app

import android.content.*
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Bundle
import android.os.Environment
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
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
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.inkprint.getLocalIp
import java.io.File
import java.text.SimpleDateFormat
import java.util.*

class MainActivity : ComponentActivity() {

    // Tick incremented on each print job — triggers file list refresh
    private var jobTick by mutableStateOf(0)

    private val jobReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == PrinterService.BROADCAST_JOB_RECEIVED) {
                jobTick++
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
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

    private fun isWifiConnected(): Boolean {
        val cm = getSystemService(CONNECTIVITY_SERVICE) as ConnectivityManager
        val caps = cm.getNetworkCapabilities(cm.activeNetwork ?: return false) ?: return false
        return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
    }

    private fun getLocalIpAddress(): String =
        try { getLocalIp() } catch (_: Exception) { "unknown" }

    private fun getStoredFiles(): List<FileEntry> {
        val dir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOCUMENTS)
            .resolve("InkPrint")
        return dir.listFiles()
            ?.filter { it.isFile }
            ?.sortedByDescending { it.lastModified() }
            ?.map { FileEntry(it.name, it.absolutePath, it.length(), it.lastModified()) }
            ?: emptyList()
    }

    private fun openFile(filePath: String) {
        try {
            val file = File(filePath)
            val uri = androidx.core.content.FileProvider.getUriForFile(
                this, "${packageName}.fileprovider", file
            )
            val mime = when (file.extension.lowercase()) {
                "pdf" -> "application/pdf"
                "ps"  -> "application/postscript"
                else  -> "*/*"
            }
            startActivity(Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mime)
                flags = Intent.FLAG_GRANT_READ_URI_PERMISSION
            })
        } catch (e: Exception) {
            android.util.Log.e("MainActivity", "Cannot open file: ${e.message}")
        }
    }

    // ── Main screen ──────────────────────────────────────────────────────────

    @Composable
    fun InkPrintScreen() {
        var isRunning by remember { mutableStateOf(false) }
        val ip            = remember { getLocalIpAddress() }
        val port          = PrinterService.DEFAULT_PORT
        val wifiOk        = remember { isWifiConnected() }
        val noNetwork     = !wifiOk || ip == "127.0.0.1" || ip == "unknown"
        val printerUrl    = "ipp://$ip:$port/ipp/print"

        val files = remember(jobTick) { getStoredFiles() }

        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Text("InkPrint", fontSize = 28.sp, fontWeight = FontWeight.Bold)
            Spacer(Modifier.height(2.dp))
            Text("IPP Virtual Printer", color = Color.Gray, fontSize = 14.sp)
            Spacer(Modifier.height(14.dp))

            // WiFi warning
            if (noNetwork) {
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF3E0))
                ) {
                    Row(modifier = Modifier.padding(12.dp), verticalAlignment = Alignment.Top) {
                        Text("⚠️", fontSize = 16.sp)
                        Spacer(Modifier.width(8.dp))
                        Text(
                            "WiFi not connected — devices on the LAN cannot reach this printer. Please connect to WiFi first.",
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
                    if (isRunning && !noNetwork) {
                        Text("$ip : $port", fontSize = 13.sp, color = Color.Gray)
                        Spacer(Modifier.height(2.dp))
                        CopyableText(printerUrl)
                    }
                }
            }

            Spacer(Modifier.height(16.dp))

            // File browser
            FileBrowserCard(files = files, onOpen = { openFile(it) })

            Spacer(Modifier.height(12.dp))

            // Collapsible help
            AddPrinterInstructionsCard(ip = ip, port = port.toString())

            Spacer(Modifier.height(16.dp))

            // Exit
            OutlinedButton(
                onClick = { exitApp() },
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = Color(0xFFB71C1C))
            ) {
                Text("Exit App", fontSize = 15.sp)
            }

            Spacer(Modifier.height(16.dp))
        }
    }
}

data class FileEntry(val name: String, val path: String, val sizeBytes: Long, val modifiedMs: Long)

// ── File browser card ────────────────────────────────────────────────────────

@Composable
fun FileBrowserCard(files: List<FileEntry>, onOpen: (String) -> Unit) {
    val dateFmt = remember { SimpleDateFormat("MM/dd HH:mm", Locale.getDefault()) }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Printed Files", fontWeight = FontWeight.Bold, fontSize = 15.sp,
                    modifier = Modifier.weight(1f))
                Text("${files.size}", color = Color.Gray, fontSize = 13.sp)
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
                    files.forEach { file ->
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
                                    file.name,
                                    fontSize = 13.sp,
                                    fontWeight = FontWeight.Medium,
                                    maxLines = 1,
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
            }
        }
    }
}

private fun fileIcon(name: String) = when (name.substringAfterLast('.').lowercase()) {
    "pdf" -> "\uD83D\uDCC4"
    "ps"  -> "\uD83D\uDDA8"
    else  -> "\uD83D\uDCC1"
}

private fun formatSize(bytes: Long): String = when {
    bytes < 1024            -> "$bytes B"
    bytes < 1024 * 1024     -> "${bytes / 1024} KB"
    else                    -> "${bytes / (1024 * 1024)} MB"
}

// ── Collapsible instructions card ────────────────────────────────────────────

@Composable
fun AddPrinterInstructionsCard(ip: String, port: String) {
    var expanded by remember { mutableStateOf(false) }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column {
            // Header — tap to expand/collapse
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
                Column(modifier = Modifier.padding(start = 14.dp, end = 14.dp, bottom = 14.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    OsSection("macOS")  { MacOsInstructions(ip, port) }
                    OsSection("Windows") { WindowsInstructions(ip, port) }
                    OsSection("Linux")  { LinuxInstructions(ip, port) }
                    OsSection("iOS")    { IosInstructions() }
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
            Text(title, fontWeight = FontWeight.SemiBold, fontSize = 14.sp,
                modifier = Modifier.weight(1f))
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
        Text("Recommended — Terminal (custom PPD)", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        Text("Forces macOS to send PDF instead of PostScript:", fontSize = 12.sp, color = Color.Gray)
        CodeBlock(
            "cat > /tmp/inkprint.ppd << 'EOF'\n" +
            "*PPD-Adobe: \"4.3\"\n" +
            "*FormatVersion: \"4.3\"\n" +
            "*LanguageVersion: English\n" +
            "*LanguageEncoding: ISOLatin1\n" +
            "*Manufacturer: \"InkPrint\"\n" +
            "*ModelName: \"InkPrint\"\n" +
            "*NickName: \"InkPrint PDF Printer\"\n" +
            "*PSVersion: \"(3010.000) 0\"\n" +
            "*LanguageLevel: \"3\"\n" +
            "*ColorDevice: False\n" +
            "*DefaultColorSpace: Gray\n" +
            "*cupsVersion: 2.2\n" +
            "*cupsFilter2: \"application/vnd.cups-pdf application/pdf 0 -\"\n" +
            "*DefaultPageSize: A4\n" +
            "*PageSize A4/A4: \"<</PageSize[595 842]>>setpagedevice\"\n" +
            "*PageSize Letter/Letter: \"<</PageSize[612 792]>>setpagedevice\"\n" +
            "*DefaultPaperDimension: A4\n" +
            "*PaperDimension A4/A4: \"595 842\"\n" +
            "*PaperDimension Letter/Letter: \"612 792\"\n" +
            "*DefaultImageableArea: A4\n" +
            "*ImageableArea A4/A4: \"0 0 595 842\"\n" +
            "*ImageableArea Letter/Letter: \"0 0 612 792\"\n" +
            "EOF"
        )
        CodeBlock("lpadmin -x InkPrint 2>/dev/null\nlpadmin -p InkPrint -E \\\n  -v ipp://$ip:$port/ipp/print \\\n  -P /tmp/inkprint.ppd")
        HorizontalDivider()
        Text("Verify", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        CodeBlock("lpstat -p InkPrint")
    }
}

@Composable
fun WindowsInstructions(ip: String, port: String) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("Automatic", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        Step(1, "Settings → Bluetooth & devices → Printers & scanners")
        Step(2, "Click Add device — InkPrint should appear automatically")
        Step(3, "Click Add device to confirm")
        HorizontalDivider()
        Text("Manual", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        Step(1, "Settings → Bluetooth & devices → Printers & scanners")
        Step(2, "Add device → \"The printer that I want isn't listed\"")
        Step(3, "Select a shared printer by name:")
        CodeBlock("http://$ip:$port/ipp/print")
        Step(4, "Follow wizard, choose Generic / Text Only driver")
    }
}

@Composable
fun LinuxInstructions(ip: String, port: String) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("CUPS command line", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        CodeBlock("sudo lpadmin -p InkPrint -E \\\n  -v ipp://$ip:$port/ipp/print \\\n  -m everywhere\n\n# Set as default (optional)\nsudo lpoptions -d InkPrint")
        HorizontalDivider()
        Text("Print test", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        CodeBlock("lp -d InkPrint /path/to/document.pdf")
    }
}

@Composable
fun IosInstructions() {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("iOS supports AirPrint natively", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        Step(1, "Make sure InkPrint service is running and your iPhone is on the same WiFi network")
        Step(2, "In any app (Safari, Files, Mail…), tap the Share button → Print")
        Step(3, "Tap Select Printer — InkPrint should appear automatically")
        Step(4, "Select it and tap Print")
        Text("Note: no configuration needed on iOS.", fontSize = 12.sp, color = Color.Gray)
    }
}

@Composable
fun AndroidInstructions(ip: String, port: String) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("Via print-capable apps", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        Step(1, "Install an IPP-capable app such as \"Print & Share\" or \"HP Smart\" from Google Play")
        Step(2, "Add a network printer using the address:")
        CodeBlock("ipp://$ip:$port/ipp/print")
        Step(3, "Select InkPrint and print any document")
        HorizontalDivider()
        Text("Via built-in Android print", fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
        Step(1, "Settings → Connected devices → Connection preferences → Printing")
        Step(2, "Add service → Default Print Service")
        Step(3, "InkPrint should appear automatically on the same WiFi network")
    }
}

// ── Shared UI helpers ─────────────────────────────────────────────────────────

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
        Text(text, fontFamily = FontFamily.Monospace, fontSize = 12.sp,
            color = Color(0xFF1565C0), modifier = Modifier.weight(1f))
        Text(if (copied) "Copied!" else "Copy", fontSize = 11.sp, color = Color.Gray)
    }
}

@Composable
fun Step(number: Int, text: String) {
    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
        Box(
            modifier = Modifier.size(22.dp).clip(RoundedCornerShape(11.dp))
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
        modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(6.dp))
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
