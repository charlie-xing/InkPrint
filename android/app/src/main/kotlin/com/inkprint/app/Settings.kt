package com.inkprint.app

import android.content.Context
import uniffi.inkprint.PrintDefaults
import uniffi.inkprint.PrinterIdentity
import java.util.UUID

/** User preferences for the printers, the EPUB printer and searchable scans, in the app's "inkprint" prefs. */
object Settings {
    private const val PREFS = "inkprint"
    private const val EPUB_PRINTER = "epub_printer"
    private const val KEEP_PDF = "epub_keep_pdf"
    private const val OCR = "epub_ocr"
    private const val SEARCHABLE_PDF = "pdf_searchable"
    private const val PDF_UUID = "pdf_printer_uuid"
    private const val EPUB_UUID = "epub_printer_uuid"
    private const val COLOR = "print_color"
    private const val MEDIA = "print_media"
    private const val UI_COLOR = "ui_color"

    /** The app's look: colour, black and white, or [Device.hasColor] decides. */
    enum class UiColor { AUTO, COLOR, MONO }

    /** Paper sizes offered as the default, as PWG keyword to label; the core lists the same set. */
    val PAPERS = listOf(
        "iso_a4_210x297mm" to "A4",
        "iso_a5_148x210mm" to "A5",
        "iso_a6_105x148mm" to "A6",
        "iso_b5_176x250mm" to "B5",
        "na_letter_8.5x11in" to "Letter",
    )

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /** Serve and advertise the "InkPrint EPUB" printer (never in the F-Droid build). */
    fun epubPrinter(context: Context) = BuildConfig.EPUB && prefs(context).getBoolean(EPUB_PRINTER, true)

    /** Deliver the received PDF next to the EPUB made from it. */
    fun keepPdf(context: Context) = prefs(context).getBoolean(KEEP_PDF, false)

    /** Recognise text on scanned pages instead of keeping them as pictures. */
    fun ocr(context: Context) = prefs(context).getBoolean(OCR, true)

    /** Give scanned PDFs from the PDF printer an invisible OCR text layer (never in the F-Droid build). */
    fun searchablePdf(context: Context) = BuildConfig.EPUB && prefs(context).getBoolean(SEARCHABLE_PDF, false)

    /** Advertise colour printers; off = greyscale only, so clients send grey documents. */
    fun color(context: Context) = prefs(context).getBoolean(COLOR, true)

    /** PWG keyword of the default paper size. */
    fun media(context: Context) = prefs(context).getString(MEDIA, null)
        ?.takeIf { m -> PAPERS.any { it.first == m } } ?: PAPERS[0].first

    fun uiColor(context: Context) = prefs(context).getString(UI_COLOR, null)
        ?.let { v -> UiColor.entries.firstOrNull { it.name == v } } ?: UiColor.AUTO

    fun setUiColor(context: Context, choice: UiColor) = prefs(context).edit().putString(UI_COLOR, choice.name).apply()

    /** Whether to draw the app in colour now. */
    fun colorUi(context: Context) = when (uiColor(context)) {
        UiColor.AUTO -> Device.hasColor
        UiColor.COLOR -> true
        UiColor.MONO -> false
    }

    fun printDefaults(context: Context) = PrintDefaults(color = color(context), media = media(context))

    /**
     * The names, UUIDs and mDNS host this install advertises. Each install
     * draws its own UUIDs once, and the names carry a short code from them
     * ("InkPrint [3F2A]"), so several devices on one network stay apart.
     */
    fun identity(context: Context): PrinterIdentity {
        val pdfUuid = storedUuid(context, PDF_UUID)
        val code = pdfUuid.take(4).uppercase()
        return PrinterIdentity(
            pdfName = "InkPrint [$code]",
            epubName = "InkPrint EPUB [$code]",
            pdfUuid = pdfUuid,
            epubUuid = storedUuid(context, EPUB_UUID),
            hostName = "inkprint-${code.lowercase()}",
        )
    }

    private fun storedUuid(context: Context, key: String): String {
        val p = prefs(context)
        return p.getString(key, null) ?: UUID.randomUUID().toString().also { p.edit().putString(key, it).apply() }
    }

    fun setSearchablePdf(context: Context, on: Boolean) = prefs(context).edit().putBoolean(SEARCHABLE_PDF, on).apply()
    fun setEpubPrinter(context: Context, on: Boolean) = prefs(context).edit().putBoolean(EPUB_PRINTER, on).apply()
    fun setKeepPdf(context: Context, on: Boolean) = prefs(context).edit().putBoolean(KEEP_PDF, on).apply()
    fun setColor(context: Context, on: Boolean) = prefs(context).edit().putBoolean(COLOR, on).apply()
    fun setMedia(context: Context, media: String) = prefs(context).edit().putString(MEDIA, media).apply()
    fun setOcr(context: Context, on: Boolean) = prefs(context).edit().putBoolean(OCR, on).apply()
}
