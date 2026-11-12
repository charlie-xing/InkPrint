package com.inkprint.app

import android.app.*
import android.content.Intent
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiManager
import android.os.Handler
import android.os.Looper
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

        private val _running = kotlinx.coroutines.flow.MutableStateFlow(false)
        /** Whether the printer server is up, for the UI. */
        val running: kotlinx.coroutines.flow.StateFlow<Boolean> = _running

        private val _networkChanges = kotlinx.coroutines.flow.MutableStateFlow(0)
        /** Bumped whenever the device's networks change, so the UI re-reads its addresses. */
        val networkChanges: kotlinx.coroutines.flow.StateFlow<Int> = _networkChanges

        /** Let a burst of network callbacks (DHCP, IPv6, a VPN) settle before acting. */
        private const val NETWORK_SETTLE_MS = 2000L
    }

    private var isRunning = false
    private var multicastLock: WifiManager.MulticastLock? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private val handler = Handler(Looper.getMainLooper())
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    /** The LAN addresses the server was started with; a change means restarting it. */
    private var startedOn: Set<String> = emptySet()

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
                unwatchNetwork()
                stopPrinterService()
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_CANCEL_CONVERSION -> {
                ConversionQueue.cancelCurrent()
                if (!isRunning) stopSelf()
                return START_NOT_STICKY
            }
            ACTION_RESTART -> restart()
            else -> startPrinterService()
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        unwatchNetwork()
        handler.removeCallbacksAndMessages(null)
        stopPrinterService()
        super.onDestroy()
    }

    private fun restart() {
        stopPrinterService()
        // The core frees the port and mDNS names asynchronously.
        handler.removeCallbacks(startRunnable)
        handler.postDelayed(startRunnable, 1000)
    }

    private val startRunnable = Runnable { startPrinterService() }

    /** Wi-Fi and Ethernet addresses: what the core picks its address and mDNS records from. */
    private fun lanAddresses(): Set<String> =
        PrinterAddresses.list().filter { it.label == "Wi-Fi" || it.label == "Ethernet" }.map { it.ip }.toSet()

    /**
     * The core reads the device's address once, when it starts: started
     * before Wi-Fi was up, or kept running across a reconnect or a new DHCP
     * lease, it would go on announcing an address nobody can reach. So
     * watch the networks and restart the server when its LAN addresses change.
     */
    private fun watchNetwork() {
        if (networkCallback != null) return
        val cm = getSystemService(ConnectivityManager::class.java) ?: return
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) = networkChanged()
            override fun onLost(network: Network) = networkChanged()
            override fun onLinkPropertiesChanged(network: Network, lp: LinkProperties) = networkChanged()
        }
        // Every network, local-only Wi-Fi and VPNs (Tailscale) included
        val request = NetworkRequest.Builder()
            .removeCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
            .removeCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
            .build()
        try {
            cm.registerNetworkCallback(request, callback)
            networkCallback = callback
        } catch (e: Exception) {
            Log.w(TAG, "Cannot watch networks: ${e.message}")
        }
    }

    private fun unwatchNetwork() {
        val callback = networkCallback ?: return
        networkCallback = null
        handler.removeCallbacks(settleNetwork)
        try {
            getSystemService(ConnectivityManager::class.java)?.unregisterNetworkCallback(callback)
        } catch (_: Exception) {}
    }

    /** Called on a binder thread, possibly many times in a row. */
    private fun networkChanged() {
        handler.removeCallbacks(settleNetwork)
        handler.postDelayed(settleNetwork, NETWORK_SETTLE_MS)
    }

    private val settleNetwork = Runnable {
        _networkChanges.value++
        val now = lanAddresses()
        if (isRunning && now != startedOn) {
            Log.i(TAG, "LAN addresses changed $startedOn -> $now: restarting the server")
            restart()
        }
    }

    private fun startPrinterService() {
        if (isRunning) return
        watchNetwork()

        val storageDir = JobStorage.jobDir(this)

        startForeground(NOTIFICATION_ID_SERVICE, buildServiceNotification(Lang.string(this, R.string.notif_starting)))

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
                identity = Settings.identity(this),
                epubEnabled = Settings.epubPrinter(this),
                defaults = Settings.printDefaults(this),
                listener = printJobListener
            )
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start server: ${e.message}")
            false
        }

        isRunning = started
        _running.value = started
        if (started) {
            startedOn = lanAddresses()
            val ip = try { getLocalIp() } catch (e: Exception) { "unknown" }
            Log.i(TAG, "InkPrint server started on $ip:$DEFAULT_PORT")
            updateNotification(Lang.string(this, R.string.notif_running_on, "$ip:$DEFAULT_PORT"))
            // EPUB jobs a killed process left half done
            if (BuildConfig.EPUB) ConversionQueue.resume(this)
        } else {
            Log.e(TAG, "Failed to start InkPrint server")
            updateNotification(Lang.string(this, R.string.notif_failed_to_start))
        }
    }

    private fun stopPrinterService() {
        if (!isRunning) return
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
        _running.value = false
        Log.i(TAG, "InkPrint server stopped")
    }

    fun onJobReceived(jobId: Int, filePath: String, fileName: String, sizeBytes: Long, output: OutputFormat) {
        Log.i(TAG, "New print job: $fileName ($sizeBytes bytes, $output) -> $filePath")

        val jobFile = java.io.File(filePath)
        // Converted in the background; the queue delivers and notifies.
        val background = when {
            output == OutputFormat.EPUB -> ConversionQueue.Kind.EPUB
            Settings.searchablePdf(this) -> ConversionQueue.Kind.SEARCHABLE
            else -> null
        }
        if (background != null) {
            ConversionQueue.enqueue(this, jobFile, background)
            sendBroadcast(Intent(BROADCAST_JOB_RECEIVED).setPackage(packageName).putExtra("file_name", fileName))
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
            .setContentTitle(Lang.string(this, R.string.notif_document_received))
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
            .addAction(android.R.drawable.ic_delete, Lang.string(this, R.string.notif_stop), stopPendingIntent)
            .build()
    }

    private fun updateNotification(status: String) {
        val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        nm.notify(NOTIFICATION_ID_SERVICE, buildServiceNotification(status))
    }

    private fun createNotificationChannels() {
        val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ID_SERVICE, Lang.string(this, R.string.channel_service), NotificationManager.IMPORTANCE_LOW)
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ID_JOBS, Lang.string(this, R.string.channel_jobs), NotificationManager.IMPORTANCE_DEFAULT)
        )
        nm.createNotificationChannel(
            NotificationChannel(ConversionQueue.CHANNEL_ID, Lang.string(this, R.string.channel_conversion), NotificationManager.IMPORTANCE_LOW)
        )
    }

    private fun formatSize(bytes: Long): String = when {
        bytes < 1024 -> "$bytes B"
        bytes < 1024 * 1024 -> "${bytes / 1024} KB"
        else -> "${bytes / (1024 * 1024)} MB"
    }
}
