package sh.zeron.android.core

import android.content.Context
import android.content.ContextWrapper
import android.net.Uri
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.AuthTokens
import uniffi.zeron_core.Credentials
import uniffi.zeron_core.authCallbackScheme
import java.util.UUID
import sh.zeron.android.ZeronApplication

/** Real Keystore migration and callback checks; no browser or network requests. */
class CredentialStoreTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    @Test fun legacyTokensMigrateToKeystoreAndRefreshSurvivesRecreation() {
        val prefix = "credentials-test-${UUID.randomUUID()}-"
        val context = object : ContextWrapper(instrumentation.targetContext) {
            override fun getSharedPreferences(name: String, mode: Int) = super.getSharedPreferences(prefix + name, mode)
        }
        val prefs = context.getSharedPreferences("credentials", Context.MODE_PRIVATE)
        val legacy = JSONObject().put("kind", "workos").put("userId", "test-user").put("orgId", "test-org")
            .put("access", "test-access").put("refresh", "test-refresh").toString()
        try {
            prefs.edit().putString("credentials", legacy).commit()
            val stored = CredentialStore(context).stored() as Credentials.WorkOs
            assertEquals("test-access", stored.tokens.accessToken)
            assertFalse(prefs.contains("credentials"))
            val sealed = requireNotNull(prefs.getString("sealed", null))
            assertFalse(sealed.contains("test-access"))
            assertNotEquals(legacy, sealed)
            CredentialStore(context).updateTokens(AuthTokens("refreshed-access", "refreshed-token"))
            val restored = CredentialStore(context).stored() as Credentials.WorkOs
            assertEquals("test-user", restored.userId)
            assertEquals("test-org", restored.orgId)
            assertEquals("refreshed-access", restored.tokens.accessToken)
            assertEquals("refreshed-token", restored.tokens.refreshToken)
        } finally { context.deleteSharedPreferences(prefix + "credentials") }
    }

    @Test fun mismatchedOAuthCallbackCannotChangeTheWorkspace() {
        val context = instrumentation.targetContext
        val prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE)
        val previous = prefs.getString("authState", null)
        val model = (context.applicationContext as ZeronApplication).model
        instrumentation.runOnMainSync {
            val client = model.client.value
            val error = model.signInError.value
            try {
                val first = Uri.parse(model.beginSignIn()).getQueryParameter("state")
                val second = Uri.parse(model.beginSignIn()).getQueryParameter("state")
                assertNotNull(first)
                assertNotEquals(first, second)
                model.handleCallback("${authCallbackScheme()}://callback?code=not-a-real-code&state=$first")
                assertSame(client, model.client.value)
                assertEquals(second, prefs.getString("authState", null))
                assertEquals("Sign-in was interrupted. Try again.", model.signInError.value)
            } finally {
                prefs.edit().putString("authState", previous).commit()
                model.signInError.value = error
            }
        }
    }
}
