package com.inkprint.app

import android.app.*
import android.content.Intent
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import uniffi.inkprint.OutputFormat
import uniffi.inkprint.PrintJobListener
import uniffi.inkprint.startServer
import uniffi.inkprint.stopServer
import uniffi.inkprint.getLocalIp

class PrinterService : Service() {

    companion object {
        const val TAG = "PrinterService"
        const val CHANNEL_ID_SERVICE = "inkprint_service"
        const val CHANNEL_ID_JOBS = "inkprint_jobs"
        const val NOTIFICATION_ID_SERVICE = 1
        const val ACTION_START = "com.inkprint.app.START"
        const val ACTION_STOP = "com.inkprint.app.STOP"
        /** Stop and start again, e.g. after the EPUB printer was switched on or off. */
        const val ACTION_RESTART = "com.inkprint.app.RESTART"
        const val ACTION_CANCEL_CONVERSION = "com.inkprint.app.CANCEL_CONVERSION"
        const val BROADCAST_JOB_RECEIVED = "com.inkprint.app.JOB_RECEIVED"
        const val DEFAULT_PORT: UShort = 6310u
    }

    private var isRunning = false
    private var multicastLock: WifiManager.MulticastLock? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var nsdManager: NsdManager? = null
    private val nsdListeners = mutableListOf<NsdManager.RegistrationListener>()

    private val printJobListener = object : PrintJobListener {
        override fun onJobReceived(jobId: UInt, filePath: String, fileName: String, sizeBytes: ULong, output: OutputFormat) {
            this@PrinterService.onJobReceived(jobId.toInt(), filePath, fileName, sizeBytes.toLong(), output)
        }
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannels()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopPrinterService()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_CANCEL_CONVERSION -> {
                ConversionQueue.cancelCurrent()
                if (!isRunning) stopSelf()
                return START_NOT_STICKY
            }
            ACTION_RESTART -> {
                stopPrinterService()
                // The core frees the port and mDNS names asynchronously.
                android.os.Handler(mainLooper).postDelayed({ startPrinterService() }, 1000)
            }
            else -> startPrinterService()
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        stopPrinterService()
        super.onDestroy()
    }

    private fun startPrinterService() {
        if (isRunning) return

        val storageDir = JobStorage.jobDir(this)

        startForeground(NOTIFICATION_ID_SERVICE, buildServiceNotification("Starting..."))

        val wifiManager = applicationContext.getSystemService(WIFI_SERVICE) as WifiManager
        multicastLock = wifiManager.createMulticastLock("inkprint_mdns").also {
            it.setReferenceCounted(true)
            it.acquire()
        }
        // Keep WiFi radio active while service runs so mDNS responses are not
        // dropped by Android power-save when the screen is off.
        @Suppress("DEPRECATION")
        wifiLock = wifiManager.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "inkprint_wifi").also {
            it.setReferenceCounted(false)
            it.acquire()
        }

        val started = try {
            startServer(
                port = DEFAULT_PORT,
                storagePath = storageDir.absolutePath,
                printerName = "InkPrint",
                epubEnabled = Settings.epubPrinter(this),
                listener = printJobListener
            )
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start server: ${e.message}")
            false
        }

        isRunning = started
        if (started) {
            val ip = try { getLocalIp() } catch (e: Exception) { "unknown" }
            Log.i(TAG, "InkPrint server started on $ip:$DEFAULT_PORT")
            updateNotification("Running on $ip:$DEFAULT_PORT")
            registerMdns(ip)
            // EPUB jobs a killed process left half done
            if (BuildConfig.EPUB) ConversionQueue.resume(this)
        } else {
            Log.e(TAG, "Failed to start InkPrint server")
            updateNotification("Failed to start")
        }
    }

    private fun registerMdns(ip: String) {
        val nsd = getSystemService(NSD_SERVICE) as NsdManager
        nsdManager = nsd
        // One service per printer; keep names, paths and UUIDs in sync with
        // PrinterProfile in the core (which advertises the same set itself).
        val printers = buildList {
            add(Triple("InkPrint", "ipp/print", "a7d4b3e2-1c5f-4d8a-9e0b-2f6c8d3a1b4e"))
            if (Settings.epubPrinter(this@PrinterService)) {
                add(Triple("InkPrint EPUB", "ipp/epub", "5c1e9f30-8b7d-4a26-b3e4-6d0f2a9c7e15"))
            }
        }
        for ((name, resource, uuid) in printers) {
            // Register _universal._sub._ipp._tcp.
            // On Android 12+ mDNSResponder (per RFC 6763 §7.1) automatically creates BOTH:
            //   • _ipp._tcp.local.              PTR → InkPrint._ipp._tcp.local.  (base, for all clients)
            //   • _universal._sub._ipp._tcp.local. PTR → InkPrint._ipp._tcp.local.  (AirPrint subtype)
            // macOS reads the _universal subtype to auto-select "AirPrint" in Add Printer.
            val info = NsdServiceInfo().apply {
                serviceName = name
                serviceType = "_universal._sub._ipp._tcp"
                port = DEFAULT_PORT.toInt()
                setAttribute("txtvers",  "1")
                setAttribute("pdl",      "application/pdf,image/urf")  // keep in sync with SUPPORTED_DOCUMENT_FORMATS in the core
                setAttribute("rp",       resource)
                setAttribute("ty",       if (resource == "ipp/epub") "InkPrint Virtual EPUB Printer" else "InkPrint Virtual PDF Printer")
                setAttribute("adminurl", "http://$ip:${DEFAULT_PORT.toInt()}/")
                setAttribute("UUID",     uuid)
                setAttribute("Color",    "F")
                setAttribute("Duplex",   "F")
                setAttribute("Fax",      "F")
                setAttribute("Scan",     "F")
                setAttribute("Copies",   "F")
                setAttribute("PaperMax", "legal-A4")
                setAttribute("note",     "E-ink reader virtual printer")
                setAttribute("URF",      "CP1,W8,RS300")
            }

            val listener = object : NsdManager.RegistrationListener {
                override fun onRegistrationFailed(si: NsdServiceInfo, err: Int) {
                    Log.e(TAG, "mDNS register of $name FAILED err=$err")
                }
                override fun onUnregistrationFailed(si: NsdServiceInfo, err: Int) {
                    Log.w(TAG, "mDNS unregister of $name failed err=$err")
                }
                override fun onServiceRegistered(si: NsdServiceInfo) {
                    Log.i(TAG, "mDNS registered: ${si.serviceName} [_universal._sub._ipp._tcp] rp=$resource")
                }
                override fun onServiceUnregistered(si: NsdServiceInfo) {
                    Log.i(TAG, "mDNS unregistered: ${si.serviceName}")
                }
            }
            try {
                nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
                nsdListeners += listener
            } catch (e: Exception) {
                Log.e(TAG, "NsdManager register of $name exception: ${e.message}")
            }
        }
    }

    private fun unregisterMdns() {
        val nsd = nsdManager ?: return
        for (l in nsdListeners) {
            try { nsd.unregisterService(l) } catch (_: Exception) {}
        }
        nsdListeners.clear()
        nsdManager = null
    }

    private fun stopPrinterService() {
        if (!isRunning) return
        unregisterMdns()
        try {
            stopServer()
        } catch (e: Exception) {
            Log.e(TAG, "Error stopping server: ${e.message}")
        }
        multicastLock?.let {
            if (it.isHeld) it.release()
            multicastLock = null
        }
        wifiLock?.let {
            if (it.isHeld) it.release()
            wifiLock = null
        }
        isRunning = false
        Log.i(TAG, "InkPrint server stopped")
    }

    fun onJobReceived(jobId: Int, filePath: String, fileName: String, sizeBytes: Long, output: OutputFormat) {
        Log.i(TAG, "New print job: $fileName ($sizeBytes bytes, $output) -> $filePath")

        val jobFile = java.io.File(filePath)
        if (output == OutputFormat.EPUB) {
            // Converted in the background; the queue delivers and notifies.
            ConversionQueue.enqueue(this, jobFile)
            sendBroadcast(Intent(BROADCAST_JOB_RECEIVED).setPackage(packageName))
            return
        }

        // Move the job into the user's save folder (or, without one, mirror it
        // into shared Documents/InkPrint/) so the device's reader app finds it.
        val savedUri = JobStorage.deliver(this, jobFile)
        if (savedUri != null) {
            Log.i(TAG, "Saved $fileName to the save folder: $savedUri")
        }

        val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        val openIntent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(
                savedUri ?: androidx.core.content.FileProvider.getUriForFile(
                    this@PrinterService, "${packageName}.fileprovider", jobFile
                ),
                JobStorage.mimeTypeOf(jobFile)
            )
            flags = Intent.FLAG_GRANT_READ_URI_PERMISSION
        }
        val pendingIntent = PendingIntent.getActivity(
            this, jobId, openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val notification = NotificationCompat.Builder(this, CHANNEL_ID_JOBS)
            .setSmallIcon(android.R.drawable.ic_menu_save)
            .setContentTitle("New document received")
            .setContentText("$fileName (${formatSize(sizeBytes)})")
            .setContentIntent(pendingIntent)
            .setAutoCancel(true)
            .build()
        nm.notify(NOTIFICATION_ID_SERVICE + jobId, notification)

        sendBroadcast(Intent(BROADCAST_JOB_RECEIVED).apply {
            setPackage(packageName)
            putExtra("job_id", jobId)
            putExtra("file_path", filePath)
            putExtra("file_name", fileName)
            putExtra("size_bytes", sizeBytes)
        })
    }

    private fun buildServiceNotification(status: String): Notification {
        val stopIntent = Intent(this, PrinterService::class.java).apply {
            action = ACTION_STOP
        }
        val stopPendingIntent = PendingIntent.getService(
            this, 0, stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Builder(this, CHANNEL_ID_SERVICE)
            .setSmallIcon(android.R.drawable.ic_menu_send)
            .setContentTitle("InkPrint")
            .setContentText(status)
            .setOngoing(true)
            .addAction(android.R.drawable.ic_delete, "Stop", stopPendingIntent)
            .build()
    }

    private fun updateNotification(status: String) {
        val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        nm.notify(NOTIFICATION_ID_SERVICE, buildServiceNotification(status))
    }

    private fun createNotificationChannels() {
        val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ID_SERVICE, "Printer Service", NotificationManager.IMPORTANCE_LOW)
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ID_JOBS, "Print Jobs", NotificationManager.IMPORTANCE_DEFAULT)
        )
        nm.createNotificationChannel(
            NotificationChannel(ConversionQueue.CHANNEL_ID, "EPUB Conversion", NotificationManager.IMPORTANCE_LOW)
        )
    }

    private fun formatSize(bytes: Long): String = when {
        bytes < 1024 -> "$bytes B"
        bytes < 1024 * 1024 -> "${bytes / 1024} KB"
        else -> "${bytes / (1024 * 1024)} MB"
    }
}
