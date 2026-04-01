package com.inkprint.app

import uniffi.inkprint.*

/**
 * Wrapper around the UniFFI-generated Rust bindings.
 */
object InkPrintLib {
    fun startServer(
        port: UShort,
        storagePath: String,
        printerName: String,
        listener: PrintJobListener? = null
    ): Boolean = startServer(port, storagePath, printerName, listener)

    fun stopServer(): Boolean = stopServer()

    fun getVersion(): String = getVersion()

    fun getLocalIp(): String = getLocalIp()
}
