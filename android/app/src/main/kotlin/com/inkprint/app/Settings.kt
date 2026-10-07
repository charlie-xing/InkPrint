package com.inkprint.app

import android.content.Context

/** User preferences for the EPUB printer, in the app's "inkprint" prefs. */
object Settings {
    private const val PREFS = "inkprint"
    private const val EPUB_PRINTER = "epub_printer"
    private const val KEEP_PDF = "epub_keep_pdf"
    private const val OCR = "epub_ocr"

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /** Serve and advertise the "InkPrint EPUB" printer (never in the F-Droid build). */
    fun epubPrinter(context: Context) = BuildConfig.EPUB && prefs(context).getBoolean(EPUB_PRINTER, true)

    /** Deliver the received PDF next to the EPUB made from it. */
    fun keepPdf(context: Context) = prefs(context).getBoolean(KEEP_PDF, false)

    /** Recognise text on scanned pages instead of keeping them as pictures. */
    fun ocr(context: Context) = prefs(context).getBoolean(OCR, true)

    fun setEpubPrinter(context: Context, on: Boolean) = prefs(context).edit().putBoolean(EPUB_PRINTER, on).apply()
    fun setKeepPdf(context: Context, on: Boolean) = prefs(context).edit().putBoolean(KEEP_PDF, on).apply()
    fun setOcr(context: Context, on: Boolean) = prefs(context).edit().putBoolean(OCR, on).apply()
}
