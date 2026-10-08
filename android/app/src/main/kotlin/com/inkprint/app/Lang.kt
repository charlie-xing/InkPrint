package com.inkprint.app

import android.content.Context
import android.content.res.Configuration
import android.os.LocaleList
import java.util.Locale

/**
 * The app's language: the system's, or one the user picked. Android's
 * per-app language setting needs API 33 and BOOX readers run older
 * versions, so a pick is applied by wrapping each Context (activity,
 * service, and the strings of background notifications) in a
 * configuration with that locale.
 */
object Lang {
    enum class Choice(val tag: String?) {
        SYSTEM(null),
        ENGLISH("en"),
        SIMPLIFIED("zh-CN"),
        TRADITIONAL("zh-TW"),
    }

    private const val PREFS = "inkprint"
    private const val KEY = "language"

    fun choice(context: Context): Choice {
        val saved = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null)
        return Choice.entries.firstOrNull { it.name == saved } ?: Choice.SYSTEM
    }

    fun set(context: Context, choice: Choice) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY, choice.name).apply()
    }

    /** [base] with the chosen language applied (unchanged for "system"). */
    fun wrap(base: Context): Context {
        val tag = choice(base).tag ?: return base
        val locale = Locale.forLanguageTag(tag)
        val config = Configuration(base.resources.configuration)
        config.setLocale(locale)
        config.setLocales(LocaleList(locale))
        return base.createConfigurationContext(config)
    }

    /** The current language's strings, for code without a wrapped Context. */
    fun string(context: Context, id: Int, vararg args: Any): String = wrap(context).getString(id, *args)
}
