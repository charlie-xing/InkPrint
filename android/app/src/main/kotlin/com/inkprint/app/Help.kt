package com.inkprint.app

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private enum class Os(val label: String) { MAC("macOS"), WINDOWS("Windows"), LINUX("Linux"), IOS("iOS"), ANDROID("Android") }

/**
 * How to add the printer(s), one system at a time: pick it at the top
 * instead of expanding sections (nothing animates or shifts on e-ink).
 */
@Composable
fun HelpTab(addresses: List<PrinterAddress>, port: String, epub: Boolean) {
    var os by rememberSaveable { mutableStateOf(Os.MAC) }
    // Which network the steps are written for; defaults to the first (Wi-Fi when present)
    var selectedLabel by rememberSaveable { mutableStateOf<String?>(null) }
    val selected = addresses.firstOrNull { it.label == selectedLabel } ?: addresses.firstOrNull()
    val ip = selected?.ip ?: stringResource(R.string.device_ip_placeholder)

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp)
    ) {
        InkHeading(stringResource(R.string.help_title))
        Text(
            stringResource(if (epub) R.string.help_intro_two_printers else R.string.help_intro_one_printer),
            fontSize = 16.sp
        )
        InkSegmented(Os.entries.map { it to it.label }, os, { os = it })
        if (addresses.size > 1) {
            Text(stringResource(R.string.help_network), fontSize = 15.sp, fontWeight = FontWeight.Bold)
            InkSegmented(addresses.map { it.label to it.label }, selected?.label, { selectedLabel = it })
        }
        when (os) {
            Os.MAC -> MacOsInstructions(ip, port, epub)
            Os.WINDOWS -> WindowsInstructions(ip, port, epub)
            Os.LINUX -> LinuxInstructions(ip, port, epub)
            Os.IOS -> IosInstructions(epub)
            Os.ANDROID -> AndroidInstructions(ip, port, epub)
        }
        Text(stringResource(R.string.help_driverless), fontSize = 15.sp, fontWeight = FontWeight.Bold)
        Text(stringResource(R.string.help_vpn), fontSize = 15.sp)
    }
}

@Composable
private fun Sub(text: String) {
    Text(text, fontSize = 17.sp, fontWeight = FontWeight.Bold)
}

@Composable
private fun MacOsInstructions(ip: String, port: String, epub: Boolean) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Sub(stringResource(R.string.help_auto_recommended))
        Step(1, stringResource(R.string.mac_step1))
        Step(2, stringResource(R.string.mac_step2))
        Step(3, stringResource(if (epub) R.string.mac_step3_two else R.string.mac_step3_one))
        Step(4, stringResource(R.string.mac_step4))
        if (epub) Step(5, stringResource(R.string.mac_step5_repeat))
        Sub(stringResource(R.string.help_manual_terminal))
        CodeBlock(
            "lpadmin -p InkPrint -E \\\n" +
                "  -v ipp://$ip:$port/ipp/print \\\n" +
                "  -m everywhere" +
                if (epub) "\n\n# EPUB\n" +
                    "lpadmin -p InkPrint-EPUB -E \\\n" +
                    "  -v ipp://$ip:$port/ipp/epub \\\n" +
                    "  -m everywhere" else ""
        )
        Text(stringResource(R.string.mac_no_postscript), fontSize = 15.sp)
    }
}

@Composable
private fun WindowsInstructions(ip: String, port: String, epub: Boolean) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Sub(stringResource(R.string.help_auto))
        Step(1, stringResource(R.string.win_step1))
        Step(2, stringResource(if (epub) R.string.win_step2_two else R.string.win_step2_one))
        Step(3, stringResource(if (epub) R.string.win_step3_two else R.string.win_step3_one))
        Text(stringResource(R.string.win_not_listed), fontSize = 15.sp)
        Sub(stringResource(R.string.help_manual_ip))
        Step(1, stringResource(R.string.win_step1))
        Step(2, stringResource(R.string.win_manual2))
        Step(3, stringResource(R.string.win_manual3))
        Step(4, stringResource(R.string.win_manual4))
        CodeBlock(ip)
        Step(5, stringResource(R.string.win_manual5, port) + if (epub) stringResource(R.string.win_manual5_epub) else "")
        Step(6, stringResource(R.string.win_manual6))
        if (epub) Step(7, stringResource(R.string.win_manual7_epub))
        Text(stringResource(R.string.win_no_generic), fontSize = 15.sp)
    }
}

@Composable
private fun LinuxInstructions(ip: String, port: String, epub: Boolean) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Sub(stringResource(R.string.linux_cups))
        Text(stringResource(R.string.linux_distros), fontSize = 15.sp)
        CodeBlock(
            "sudo lpadmin -p InkPrint -E \\\n" +
                "  -v ipp://$ip:$port/ipp/print \\\n" +
                "  -m everywhere\n\n" +
                (if (epub) "# EPUB\n" +
                    "sudo lpadmin -p InkPrint-EPUB -E \\\n" +
                    "  -v ipp://$ip:$port/ipp/epub \\\n" +
                    "  -m everywhere\n\n" else "") +
                "lp -d InkPrint /path/to/document.pdf"
        )
        Sub(stringResource(R.string.linux_gui))
        Step(1, stringResource(R.string.linux_gui1))
        Step(2, stringResource(R.string.linux_gui2))
        CodeBlock("ipp://$ip:$port/ipp/print" + if (epub) "\nipp://$ip:$port/ipp/epub" else "")
        Step(3, stringResource(R.string.linux_gui3))
        Text(stringResource(R.string.linux_no_generic), fontSize = 15.sp)
    }
}

@Composable
private fun IosInstructions(epub: Boolean) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Sub(stringResource(R.string.ios_airprint))
        Text(stringResource(R.string.ios_no_setup), fontSize = 15.sp)
        Step(1, stringResource(R.string.ios_step1))
        Step(2, stringResource(R.string.ios_step2))
        Step(3, stringResource(R.string.ios_step3))
        Step(4, stringResource(R.string.ios_step4))
        Step(5, stringResource(if (epub) R.string.ios_step5_two else R.string.ios_step5_one))
        Step(6, stringResource(if (epub) R.string.ios_step6_two else R.string.ios_step6_one))
        Sub(stringResource(R.string.ios_hotspot))
        Text(stringResource(R.string.ios_hotspot_text), fontSize = 15.sp)
    }
}

@Composable
private fun AndroidInstructions(ip: String, port: String, epub: Boolean) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Sub(stringResource(R.string.android_builtin))
        Step(1, stringResource(R.string.android_step1))
        Step(2, stringResource(R.string.android_step2))
        Step(3, stringResource(R.string.android_step3))
        Step(4, stringResource(if (epub) R.string.android_step4_two else R.string.android_step4_one))
        Step(5, stringResource(if (epub) R.string.android_step5_two else R.string.android_step5_one))
        Sub(stringResource(R.string.help_manual_ip))
        Step(1, stringResource(R.string.android_manual1))
        Step(2, stringResource(R.string.android_manual2))
        CodeBlock("$ip:$port")
        if (epub) Text(stringResource(R.string.android_manual_epub), fontSize = 15.sp)
    }
}
