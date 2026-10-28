package sh.zeron.android.update

import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.*
import org.junit.Test

class UpdateProviderManifestTest {
    @Test fun updateAndSharingProvidersHaveDistinctComponentsAndPrivateGrants() {
        val document = DocumentBuilderFactory.newInstance().apply { isNamespaceAware = true }
            .newDocumentBuilder().parse(File("src/main/AndroidManifest.xml"))
        val providers = document.getElementsByTagName("provider")
        val android = "http://schemas.android.com/apk/res/android"
        val names = mutableSetOf<String>()
        var updates = false
        var sharing = false
        for (index in 0 until providers.length) {
            val provider = providers.item(index) as org.w3c.dom.Element
            val name = provider.getAttributeNS(android, "name")
            assertTrue("Provider class registered twice: $name", names.add(name))
            when (provider.getAttributeNS(android, "authorities")) {
                "\${applicationId}.updates" -> {
                    updates = true
                    assertEquals(UpdateFileProvider::class.java.name, name)
                    assertEquals("false", provider.getAttributeNS(android, "exported"))
                    assertEquals("true", provider.getAttributeNS(android, "grantUriPermissions"))
                    val metadata = provider.getElementsByTagName("meta-data").item(0) as org.w3c.dom.Element
                    assertEquals("@xml/update_paths", metadata.getAttributeNS(android, "resource"))
                }
                "\${applicationId}.files" -> sharing = true
            }
        }
        assertTrue("Missing update provider", updates)
        assertTrue("Missing sharing provider", sharing)
    }
}
