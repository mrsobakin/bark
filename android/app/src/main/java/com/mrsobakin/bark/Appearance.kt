package com.mrsobakin.bark

import android.content.Context
import com.google.android.material.color.DynamicColors

object Appearance {
    const val PREF_DYNAMIC_COLORS = "appearance_dynamic_colors"

    private const val PREFS = "bark"

    fun dynamicColors(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getBoolean(PREF_DYNAMIC_COLORS, true)

    fun wrap(context: Context): Context =
        if (dynamicColors(context)) DynamicColors.wrapContextIfAvailable(context) else context
}
