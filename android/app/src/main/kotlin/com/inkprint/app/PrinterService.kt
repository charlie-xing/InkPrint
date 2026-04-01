package com.inkprint.app

import android.app.*
import android.content.Intent
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.IBinder
import android.os.Environment
import android.util.Log
import androidx.core.app.NotificationCompat
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
        const val BROADCAST_JOB_RECEIVED = "com.inkprint.app.JOB_RECEIVED"
        const val DEFAULT_PORT: UShort = 6310u
    }

    private var isRunning = false
    private var multicastLock: WifiManager.MulticastLock? = null
    private var nsdManager: NsdManager? = null
    private val nsdListeners = mutableListOf<NsdManager.RegistrationListener>()

    private val printJobListener = object : PrintJobListener {
        override fun onJobReceived(jobId: UInt, filePath: String, fileName: String, sizeBytes: ULong) {
            this@PrinterService.onJobReceived(jobId.toInt(), filePath, fileName, sizeBytes.toLong())
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

        val storageDir = Environment.getExternalStoragePublicDirectory(
            Environment.DIRECTORY_DOCUMENTS
        ).resolve("InkPrint").also { it.mkdirs() }

        startForeground(NOTIFICATION_ID_SERVICE, buildServiceNotification("Starting..."))

        val wifiManager = applicationContext.getSystemService(WIFI_SERVICE) as WifiManager
        multicastLock = wifiManager.createMulticastLock("inkprint_mdns").also {
            it.setReferenceCounted(true)
            it.acquire()
        }

        val started = try {
            startServer(
                port = DEFAULT_PORT,
                storagePath = storageDir.absolutePath,
                printerName = "InkPrint",
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
            registerMdnsServices(ip)
        } else {
            Log.e(TAG, "Failed to start InkPrint server")
            updateNotification("Failed to start")
        }
    }

    private fun registerMdnsServices(ip: String) {
        val nsd = getSystemService(NSD_SERVICE) as NsdManager
        nsdManager = nsd

        val txtAttrs = mapOf(
            "txtvers"  to "1",
            "pdl"      to "application/pdf,image/urf,image/pwg-raster,image/jpeg",
            "rp"       to "ipp/print",
            "ty"       to "InkPrint Virtual Printer",
            "adminurl" to "http://$ip:${DEFAULT_PORT.toInt()}/",
            "UUID"     to "a7d4b3e2-1c5f-4d8a-9e0b-2f6c8d3a1b4e",
            "Color"    to "F",
            "Duplex"   to "F",
            "Fax"      to "F",
            "Scan"     to "F",
            "Copies"   to "F",
            "PaperMax" to "legal-A4",
            "note"     to "E-ink reader virtual printer",
            "URF"      to "CP1,W8,RS300",
        )

        // Register _ipp._tcp as the primary service type.
        // Android NsdManager (API < 33) does NOT automatically create a base _ipp._tcp PTR record
        // when a subtype like _universal._sub._ipp._tcp is registered — so we register the base
        // type explicitly. This is what dns-sd -B _ipp._tcp and most print clients browse for.
        // On API 33+ devices we also register the _universal subtype for AirPrint/macOS.
        val baseType = "_ipp._tcp"
        val baseInfo = NsdServiceInfo().apply {
            serviceName = "InkPrint"
            serviceType = baseType
            port = DEFAULT_PORT.toInt()
            txtAttrs.forEach { (k, v) -> setAttribute(k, v) }
        }
        val baseListener = makeListener(baseType)
        nsdListeners.add(baseListener)
        try {
            nsd.registerService(baseInfo, NsdManager.PROTOCOL_DNS_SD, baseListener)
        } catch (e: Exception) {
            Log.e(TAG, "NsdManager register exception [$baseType]: ${e.message}")
            nsdListeners.remove(baseListener)
        }

        // On Android 13+ (API 33), also register the _universal._sub._ipp._tcp subtype so
        // macOS discovers this printer as "AirPrint" automatically.
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            val subtypeInfo = NsdServiceInfo().apply {
                serviceName = "InkPrint"
                serviceType = "_universal._sub._ipp._tcp"
                port = DEFAULT_PORT.toInt()
                txtAttrs.forEach { (k, v) -> setAttribute(k, v) }
            }
            val subtypeListener = makeListener("_universal._sub._ipp._tcp")
            nsdListeners.add(subtypeListener)
            try {
                nsd.registerService(subtypeInfo, NsdManager.PROTOCOL_DNS_SD, subtypeListener)
            } catch (e: Exception) {
                Log.e(TAG, "NsdManager register exception [_universal._sub._ipp._tcp]: ${e.message}")
                nsdListeners.remove(subtypeListener)
            }
        }
    }

    private fun makeListener(serviceType: String) = object : NsdManager.RegistrationListener {
        override fun onRegistrationFailed(si: NsdServiceInfo, err: Int) {
            Log.e(TAG, "mDNS register FAILED [$serviceType] err=$err")
        }
        override fun onUnregistrationFailed(si: NsdServiceInfo, err: Int) {
            Log.w(TAG, "mDNS unregister failed [$serviceType] err=$err")
        }
        override fun onServiceRegistered(si: NsdServiceInfo) {
            Log.i(TAG, "mDNS registered: ${si.serviceName} [$serviceType]")
        }
        override fun onServiceUnregistered(si: NsdServiceInfo) {
            Log.i(TAG, "mDNS unregistered: ${si.serviceName} [$serviceType]")
        }
    }

    private fun unregisterMdnsServices() {
        val nsd = nsdManager ?: return
        nsdListeners.forEach { listener ->
            try { nsd.unregisterService(listener) } catch (_: Exception) {}
        }
        nsdListeners.clear()
        nsdManager = null
    }

    private fun stopPrinterService() {
        if (!isRunning) return
        unregisterMdnsServices()
        try {
            stopServer()
        } catch (e: Exception) {
            Log.e(TAG, "Error stopping server: ${e.message}")
        }
        multicastLock?.let {
            if (it.isHeld) it.release()
            multicastLock = null
        }
        isRunning = false
        Log.i(TAG, "InkPrint server stopped")
    }

    fun onJobReceived(jobId: Int, filePath: String, fileName: String, sizeBytes: Long) {
        Log.i(TAG, "New print job: $fileName ($sizeBytes bytes) -> $filePath")

        val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        val openIntent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(
                androidx.core.content.FileProvider.getUriForFile(
                    this@PrinterService, "${packageName}.fileprovider", java.io.File(filePath)
                ),
                "application/pdf"
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
    }

    private fun formatSize(bytes: Long): String = when {
        bytes < 1024 -> "$bytes B"
        bytes < 1024 * 1024 -> "${bytes / 1024} KB"
        else -> "${bytes / (1024 * 1024)} MB"
    }
}
