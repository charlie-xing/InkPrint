package com.inkprint.app

import android.app.Application

class InkPrintApplication : Application() {
    override fun onCreate() {
        // Tell UniFFI to load our library under its actual filename.
        // The UDL namespace is "inkprint", so uniffi defaults to loading "libuniffi_inkprint.so".
        // Our compiled library is "libinkprint_core.so", so we override it here
        // BEFORE any uniffi.inkprint.* code is first accessed.
        System.setProperty("uniffi.component.inkprint.libraryOverride", "inkprint_core")
        super.onCreate()
    }
}
