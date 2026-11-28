/*
 * joma: Omarchy's themes on the phone, switched at runtime (android/tools/themes.py makes
 * the styles and JomaPaletteTable). The chosen one is applied to every activity before
 * it draws; "same as my Omarchy desktop" follows the theme your desktop Joma sends to
 * your own devices (an account message, application/x-joma-theme+json).
 */
package cx.ring.joma

import android.app.Activity
import android.app.Application
import android.content.Context
import android.content.res.ColorStateList
import android.os.Bundle
import android.util.TypedValue
import android.view.View
import android.widget.TextView
import androidx.appcompat.app.AppCompatDelegate
import androidx.core.content.ContextCompat
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import cx.ring.R
import cx.ring.application.JamiApplication
import org.json.JSONObject

object JomaPalette {
    private const val PREFS = "joma_theme"
    /** A theme's name, "desktop" (follow your Omarchy desktop), or "" (the build's). */
    private const val CHOICE = "choice"
    /** The last theme your desktop sent. */
    private const val DESKTOP = "desktop_theme"

    private fun prefs(c: Context) = c.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun choice(c: Context): String = prefs(c).getString(CHOICE, "") ?: ""

    /** The theme in use: the chosen one, or the desktop's when following it. */
    fun current(c: Context): Theme? {
        val name = when (val ch = choice(c)) {
            "desktop" -> prefs(c).getString(DESKTOP, "") ?: ""
            else -> ch
        }
        return JomaPaletteTable.themes.firstOrNull { it.name == name }
    }

    /** Light or dark from the theme (null: no theme chosen, the dark switch decides). */
    fun isDark(c: Context): Boolean? = current(c)?.dark

    fun install(app: Application) {
        current(app)?.let { app.theme.applyStyle(it.style, true) }
        app.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
            override fun onActivityPreCreated(activity: Activity, savedInstanceState: Bundle?) {
                current(activity)?.let { activity.theme.applyStyle(it.style, true) }
            }
            override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) { live.add(activity) }
            override fun onActivityStarted(activity: Activity) {}
            override fun onActivityResumed(activity: Activity) {}
            override fun onActivityPaused(activity: Activity) {}
            override fun onActivityStopped(activity: Activity) {}
            override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
            override fun onActivityDestroyed(activity: Activity) { live.remove(activity) }
        })
        applyNight(app)
        (app as? JamiApplication)?.let { j ->
            try {
                j.mAccountService.jomaTheme = { accountId, from, json -> fromDesktop(app, accountId, from, json) }
            } catch (_: UninitializedPropertyAccessException) {
                // Injected later: hooked again from the settings row.
            }
        }
    }

    private fun applyNight(c: Context) {
        val dark = isDark(c) ?: return
        AppCompatDelegate.setDefaultNightMode(if (dark) AppCompatDelegate.MODE_NIGHT_YES else AppCompatDelegate.MODE_NIGHT_NO)
    }

    /** A colour role through the theme (the build's value when no theme sets it). */
    @JvmStatic
    fun color(c: Context, id: Int): Int {
        val attr = JomaPaletteTable.attrOf[id] ?: return ContextCompat.getColor(c, id)
        val v = TypedValue()
        if (c.theme.resolveAttribute(attr, v, true) && v.type >= TypedValue.TYPE_FIRST_COLOR_INT && v.type <= TypedValue.TYPE_LAST_COLOR_INT) return v.data
        return ContextCompat.getColor(c, id)
    }

    @JvmStatic
    fun colors(c: Context, id: Int): ColorStateList =
        if (JomaPaletteTable.attrOf.containsKey(id)) ColorStateList.valueOf(color(c, id)) else ContextCompat.getColorStateList(c, id)!!

    /** Your desktop's theme arrived: keep it, and switch if following it. */
    fun fromDesktop(c: Context, accountId: String, from: String, json: String) {
        val j = c.applicationContext as? JamiApplication ?: return
        val own = try { j.mAccountService.getAccount(accountId)?.username } catch (_: Exception) { null }
        // Only from our own account (another of our devices).
        if (own == null || !from.contains(own.removePrefix("jami:").removePrefix("ring:"))) return
        val name = try { JSONObject(json).optString("name") } catch (_: Exception) { "" }
        if (name.isEmpty() || JomaPaletteTable.themes.none { it.name == name }) return
        val p = prefs(c)
        if (p.getString(DESKTOP, "") == name) return
        p.edit().putString(DESKTOP, name).apply()
        if (choice(c) == "desktop") restart(c)
    }

    private fun restart(c: Context) {
        android.os.Handler(c.mainLooper).post {
            applyNight(c)
            (c.applicationContext as? JamiApplication)?.let { app -> current(app)?.let { app.theme.applyStyle(it.style, true) } }
            live.toList().forEach { it.recreate() }
        }
    }

    private val live = mutableListOf<Activity>()

    private fun label(c: Context, name: String): String = when (name) {
        "" -> c.getString(R.string.joma_theme_build)
        "desktop" -> c.getString(R.string.joma_theme_desktop) + (prefs(c).getString(DESKTOP, "")?.takeIf { it.isNotEmpty() }?.let { " · $it" } ?: "")
        else -> name
    }

    /** The settings page's theme row: shows the theme, opens the list. */
    fun bindRow(activity: Activity, row: View, value: TextView) {
        (activity.application as? JamiApplication)?.let { j ->
            try { j.mAccountService.jomaTheme = { a, f, json -> fromDesktop(activity.applicationContext, a, f, json) } } catch (_: Exception) {}
        }
        value.text = label(activity, choice(activity))
        row.setOnClickListener {
            val names = listOf("desktop") + JomaPaletteTable.themes.map { it.name } + listOf("")
            val labels = names.map { label(activity, it) }.toTypedArray()
            MaterialAlertDialogBuilder(activity)
                .setTitle(R.string.joma_theme)
                .setSingleChoiceItems(labels, names.indexOf(choice(activity)).coerceAtLeast(0)) { d, i ->
                    prefs(activity).edit().putString(CHOICE, names[i]).apply()
                    value.text = label(activity, names[i])
                    d.dismiss()
                    applyNight(activity)
                    activity.application.theme.applyStyle((current(activity) ?: return@setSingleChoiceItems).style, true)
                    activity.recreate()
                }
                .setNegativeButton(android.R.string.cancel, null)
                .show()
        }
    }
}
