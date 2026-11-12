package com.inkprint.app

import android.os.Build

/**
 * What kind of screen the app is on, for choosing between the colour and
 * the black-and-white look. Android has no API that says "e-ink", so this
 * goes by maker and model; the user can override it in Settings.
 */
object Device {

    /** Makers whose Android devices are e-paper readers, notebooks or phones. */
    private val EINK_MAKERS = listOf(
        "onyx", "boox", "pocketbook", "bigme", "meebook", "boyue", "likebook",
        "tolino", "kobo", "remarkable", "ratta", "supernote", "dasung", "moaan",
        "inkpalm", "hanvon", "ireader", "guoyue", "xrz",
    )

    /**
     * Colour e-paper (Kaleido, Gallery) models: "Go Color 7", "InkPad Color",
     * and BOOX's C suffix — "NoteAir3C", "Tab Ultra C Pro", "Tab Mini C".
     */
    private val COLOR_MODEL = Regex("color|colour|kaleido|gallery|c$|cc$|cpro$|cplus$")

    val isEink: Boolean by lazy {
        val maker = (Build.MANUFACTURER + " " + Build.BRAND).lowercase()
        EINK_MAKERS.any { it in maker }
    }

    val isColorEink: Boolean by lazy {
        isEink && listOf(Build.MODEL, Build.DEVICE, Build.PRODUCT).any {
            COLOR_MODEL.containsMatchIn(it.lowercase().filter(Char::isLetterOrDigit))
        }
    }

    /** Whether the screen shows colour: everything except black-and-white e-paper. */
    val hasColor: Boolean get() = !isEink || isColorEink
}
