/*
 * joma: restore an account from your GitHub backup on the phone (plan A1; discussion
 * 2026-11-26-github, question 20). The same steps as the desktop's restore screen:
 *
 * 1. sign in with Joma's GitHub App (the device flow: a code to type at
 *    github.com/login/device; the token stays in memory);
 * 2. read index.json of the private repo <you>/joma-backup and pick an account;
 * 3. type the backup's password: the file is decrypted here (Argon2id, then
 *    ChaCha20-Poly1305, the format of crates/joma-backup) into Jami's own account
 *    archive, which Jami's "import from backup" then opens (no archive password).
 */
package cx.ring.joma

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.text.InputType
import android.util.Base64
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import cx.ring.R
import cx.ring.account.AccountWizardActivity
import cx.ring.account.ImportArchiveReceiverActivity
import org.bouncycastle.crypto.generators.Argon2BytesGenerator
import org.bouncycastle.crypto.params.Argon2Parameters
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder
import javax.crypto.Cipher
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec
import kotlin.concurrent.thread

class JomaRestoreActivity : AppCompatActivity() {
    private lateinit var box: LinearLayout
    @Volatile private var token: String? = null
    @Volatile private var stopped = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pad = (16 * resources.displayMetrics.density).toInt()
        box = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad * 2, pad, pad)
        }
        setContentView(ScrollView(this).apply { addView(box); fitsSystemWindows = true })
        title = getString(R.string.joma_restore_title)
        // Tests (debug builds): a backup file pushed to the phone, without GitHub.
        val local = intent.getStringExtra("joma_backup_file")
        if (local != null && cx.ring.BuildConfig.DEBUG) {
            localFile = File(local)
            askPassword(JSONObject().put("label", localFile!!.name).put("jami", localFile!!.nameWithoutExtension).put("file", ""))
            return
        }
        signIn()
    }

    override fun onDestroy() {
        stopped = true
        super.onDestroy()
    }

    private fun clear() = box.removeAllViews()

    private fun text(s: String, size: Float = 15f, quiet: Boolean = false): TextView = TextView(this).apply {
        text = s
        textSize = size
        setPadding(0, 8, 0, 8)
        setTextColor(JomaPalette.color(context, if (quiet) R.color.textColorSecondary else R.color.text_color))
        box.addView(this)
    }

    private fun button(s: String, on: () -> Unit): Button = Button(this).apply {
        text = s
        isAllCaps = false
        setOnClickListener { on() }
        box.addView(this)
    }

    private fun busy(s: String) {
        clear()
        text(s, quiet = true)
        box.addView(ProgressBar(this).apply { isIndeterminate = true })
    }

    private fun fail(s: String, retry: () -> Unit) {
        clear()
        text(getString(R.string.joma_restore_title), 22f)
        text(s)
        button(getString(R.string.joma_restore_again)) { retry() }
    }

    // ---- 1. Signing in -------------------------------------------------------------

    private fun post(url: String, form: Map<String, String>): JSONObject {
        val c = URL(url).openConnection() as HttpURLConnection
        c.requestMethod = "POST"
        c.doOutput = true
        c.setRequestProperty("Accept", "application/json")
        c.setRequestProperty("User-Agent", "joma-android")
        c.outputStream.use { o -> o.write(form.entries.joinToString("&") { (k, v) -> "$k=" + URLEncoder.encode(v, "UTF-8") }.toByteArray()) }
        return JSONObject(c.inputStream.bufferedReader().readText())
    }

    private fun api(path: String, raw: Boolean = false): ByteArray? {
        val c = URL("https://api.github.com/$path").openConnection() as HttpURLConnection
        c.setRequestProperty("Authorization", "Bearer $token")
        c.setRequestProperty("Accept", if (raw) "application/vnd.github.raw" else "application/vnd.github+json")
        c.setRequestProperty("User-Agent", "joma-android")
        return when (c.responseCode) {
            in 200..299 -> c.inputStream.readBytes()
            404 -> null
            401 -> throw Exception(getString(R.string.joma_restore_signed_out))
            else -> throw Exception("GitHub answered ${c.responseCode}")
        }
    }

    private fun signIn() {
        busy(getString(R.string.joma_restore_asking))
        val id = getString(R.string.joma_github_client_id)
        thread {
            try {
                val code = post("https://github.com/login/device/code", mapOf("client_id" to id))
                if (code.has("error")) throw Exception(explain(code))
                runOnUiThread { showCode(code) }
                var every = code.optLong("interval", 5).coerceAtLeast(5)
                val until = System.currentTimeMillis() + code.optLong("expires_in", 900) * 1000
                while (!stopped && System.currentTimeMillis() < until) {
                    Thread.sleep(every * 1000)
                    val r = post("https://github.com/login/oauth/access_token", mapOf(
                        "client_id" to id, "device_code" to code.getString("device_code"),
                        "grant_type" to "urn:ietf:params:oauth:grant-type:device_code"))
                    if (r.has("access_token")) {
                        token = r.getString("access_token")
                        runOnUiThread { list() }
                        return@thread
                    }
                    when (r.optString("error")) {
                        "authorization_pending" -> {}
                        "slow_down" -> every = r.optLong("interval", every + 5)
                        else -> throw Exception(explain(r))
                    }
                }
                if (!stopped) throw Exception(getString(R.string.joma_restore_expired))
            } catch (e: Exception) {
                if (!stopped) runOnUiThread { fail(e.message ?: e.toString()) { signIn() } }
            }
        }
    }

    private fun explain(r: JSONObject): String = when (r.optString("error")) {
        "device_flow_disabled" -> "Joma's GitHub App has the device flow off (its settings: Enable Device Flow)."
        "access_denied" -> "You cancelled the sign-in on GitHub."
        "expired_token" -> getString(R.string.joma_restore_expired)
        else -> "GitHub: ${r.optString("error")} ${r.optString("error_description")}".trim()
    }

    private fun showCode(code: JSONObject) {
        clear()
        val user = code.getString("user_code")
        val uri = code.getString("verification_uri")
        text(getString(R.string.joma_restore_title), 22f)
        text(getString(R.string.joma_restore_code_intro, uri))
        text(user, 30f).apply { gravity = Gravity.CENTER; setTextIsSelectable(true) }
        button(getString(R.string.joma_restore_open)) {
            (getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("code", user))
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
        }
        text(getString(R.string.joma_restore_scope), 13f, quiet = true)
        text(getString(R.string.joma_restore_waiting), 13f, quiet = true)
    }

    // ---- 2. The accounts in the backup ------------------------------------------------

    private var login = ""

    private fun list() {
        busy(getString(R.string.joma_restore_reading))
        thread {
            try {
                login = JSONObject(String(api("user")!!)).getString("login")
                val index = api("repos/$login/joma-backup/contents/index.json", raw = true)
                    ?: throw Exception(getString(R.string.joma_restore_no_repo, login))
                val accounts: JSONArray = JSONObject(String(index)).optJSONArray("accounts") ?: JSONArray()
                runOnUiThread { showList(accounts) }
            } catch (e: Exception) {
                runOnUiThread { fail(e.message ?: e.toString()) { list() } }
            }
        }
    }

    private fun showList(accounts: JSONArray) {
        clear()
        text(getString(R.string.joma_restore_title), 22f)
        text("github.com/$login/joma-backup", 13f, quiet = true)
        if (accounts.length() == 0) text(getString(R.string.joma_restore_empty))
        for (i in 0 until accounts.length()) {
            val a = accounts.getJSONObject(i)
            val label = a.optString("label").ifEmpty { a.getString("jami").take(8) }
            button("$label · ${a.getString("jami").take(8)}…") { askPassword(a) }
        }
    }

    // ---- 3. The password, decrypting, importing -----------------------------------------

    private fun askPassword(entry: JSONObject) {
        clear()
        text(getString(R.string.joma_restore_title), 22f)
        text(entry.optString("label"))
        val field = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            hint = getString(R.string.joma_restore_password)
            box.addView(this)
        }
        text(getString(R.string.joma_restore_password_note), 13f, quiet = true)
        button(getString(R.string.joma_restore_go)) {
            val pw = field.text.toString()
            if (pw.isNotEmpty()) restore(entry, pw)
        }
    }

    private fun restore(entry: JSONObject, password: String) {
        busy(getString(R.string.joma_restore_working))
        thread {
            try {
                val data = localFile?.readBytes() ?: api("repos/$login/joma-backup/contents/${entry.getString("file")}", raw = true)
                    ?: throw Exception("${entry.getString("file")} is in the index but not in the repo")
                val archive = open(data, password)
                val dir = File(cacheDir, "joma-restore").apply { mkdirs() }
                val out = File(dir, "${entry.getString("jami")}.gz")
                out.writeBytes(archive)
                runOnUiThread {
                    startActivity(Intent(this, AccountWizardActivity::class.java)
                        .putExtra(ImportArchiveReceiverActivity.EXTRA_ARCHIVE_PATH, out.absolutePath))
                    finish()
                }
            } catch (e: WrongPassword) {
                runOnUiThread { fail(getString(R.string.joma_restore_wrong)) { askPassword(entry) } }
            } catch (e: Exception) {
                runOnUiThread { fail(e.message ?: e.toString()) { askPassword(entry) } }
            }
        }
    }

    private var localFile: File? = null

    class WrongPassword : Exception()

    companion object {
        private val MAGIC = "JOMA-BACKUP 1\n".toByteArray()

        /** crates/joma-backup's `open`: the header line, Argon2id, ChaCha20-Poly1305. */
        fun open(data: ByteArray, password: String): ByteArray {
            require(data.size > MAGIC.size && data.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) { "not a Joma backup" }
            val end = (MAGIC.size until data.size).firstOrNull { data[it] == '\n'.code.toByte() } ?: throw IllegalArgumentException("damaged backup")
            val headerBytes = data.copyOfRange(MAGIC.size, end)
            val h = JSONObject(String(headerBytes))
            require(h.optString("kdf") == "argon2id" && h.optString("cipher") == "chacha20-poly1305") { "unknown backup format" }
            val salt = Base64.decode(h.getString("salt"), Base64.DEFAULT)
            val nonce = Base64.decode(h.getString("nonce"), Base64.DEFAULT)
            val params = Argon2Parameters.Builder(Argon2Parameters.ARGON2_id)
                .withVersion(Argon2Parameters.ARGON2_VERSION_13)
                .withMemoryAsKB(h.getInt("m_kib"))
                .withIterations(h.getInt("t"))
                .withParallelism(h.optInt("p", 1))
                .withSalt(salt)
                .build()
            val key = ByteArray(32)
            Argon2BytesGenerator().apply { init(params) }.generateBytes(password.toByteArray(Charsets.UTF_8), key)
            val cipher = Cipher.getInstance("ChaCha20-Poly1305")
            cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "ChaCha20"), IvParameterSpec(nonce))
            cipher.updateAAD(headerBytes)
            return try {
                cipher.doFinal(data.copyOfRange(end + 1, data.size))
            } catch (e: javax.crypto.AEADBadTagException) {
                throw WrongPassword()
            }
        }
    }
}
