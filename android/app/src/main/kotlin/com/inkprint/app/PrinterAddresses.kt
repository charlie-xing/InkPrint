package com.inkprint.app

import java.net.Inet4Address
import java.net.NetworkInterface

/** One way to reach the printer: the network it's on and the IPv4 address there. */
data class PrinterAddress(val label: String, val ip: String) {
    fun printerUrl(port: Int) = "ipp://$ip:$port/ipp/print"
}

/**
 * Addresses other devices can use to reach this printer. The server listens
 * on every interface, so this only decides what the UI offers:
 *
 *  - `wlan*`                         → Wi-Fi
 *  - `tun*` in 100.64.0.0/10         → Tailscale (its CGNAT range)
 *  - other `tun*`, `ppp*`            → VPN
 *  - `eth*`                          → Ethernet
 *
 * Cellular (`rmnet*`, `ccmni*`, ...) and anything else is left out: it is not
 * reachable from other devices in practice. So are `ipsec*` VPNs: on phones
 * those are outbound privacy VPNs (e.g. Google's), which also hand out
 * 100.64/10 addresses but never accept incoming connections.
 */
object PrinterAddresses {

    fun list(): List<PrinterAddress> {
        val interfaces = try {
            NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()
        } catch (_: Exception) {
            emptyList()
        }
        return interfaces
            .filter { runCatching { it.isUp && !it.isLoopback }.getOrDefault(false) }
            .flatMap { nif ->
                nif.inetAddresses.toList()
                    .filterIsInstance<Inet4Address>()
                    .mapNotNull { addr -> labelFor(nif.name, addr)?.let { PrinterAddress(it, addr.hostAddress!!) } }
            }
            .sortedBy { ORDER.indexOf(it.label) }
    }

    private val ORDER = listOf("Wi-Fi", "Ethernet", "Tailscale", "VPN")

    private fun labelFor(name: String, addr: Inet4Address): String? = when {
        name.startsWith("wlan") -> "Wi-Fi"
        name.startsWith("eth") -> "Ethernet"
        name.startsWith("tun") && isTailscale(addr) -> "Tailscale"
        name.startsWith("tun") || name.startsWith("ppp") -> "VPN"
        else -> null
    }

    /** 100.64.0.0/10: first octet 100, second octet 64..127. */
    private fun isTailscale(addr: Inet4Address): Boolean {
        val b = addr.address
        return (b[0].toInt() and 0xFF) == 100 && (b[1].toInt() and 0xC0) == 64
    }
}
