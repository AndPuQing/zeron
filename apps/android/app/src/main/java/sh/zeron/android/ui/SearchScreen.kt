package sh.zeron.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.GlassScene
import sh.zeron.android.design.GlassSurface
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons

@Composable
fun SearchScreen(model: AppModel, onBack: () -> Unit, onOpen: (String) -> Unit) {
    val client by model.client.collectAsState()
    val workspace by model.workspace.collectAsState()
    var query by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val results = remember(query, workspace) {
        val q = query.trim()
        if (q.isEmpty()) workspace?.front?.recent.orEmpty() else client?.searchIncludingChildren(q, 60u)?.map { it.session }.orEmpty()
    }
    val density = LocalDensity.current
    var searchHeight by remember { mutableStateOf(100.dp) }
    GlassScene(
        modifier = Modifier.fillMaxSize().imePadding(),
        background = { source ->
            LazyColumn(
                source.fillMaxSize().background(MaterialTheme.colorScheme.background),
                contentPadding = PaddingValues(top = searchHeight, bottom = 32.dp),
                verticalArrangement = Arrangement.spacedBy(0.dp),
            ) {
                item {
                    Text(
                        if (query.isBlank()) "Recent" else "${results.size} results",
                        style = MaterialTheme.typography.titleSmallEmphasized,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = 28.dp, top = 8.dp, bottom = 6.dp),
                    )
                }
                itemsIndexed(results, key = { _, r -> r.id }) { i, row ->
                    Box(Modifier.padding(horizontal = 8.dp)) {
                        SessionItem(row, i, results.size, model, onOpen) { model.archive(it.id) }
                    }
                }
            }
        },
    ) {
        Row(
            Modifier.onSizeChanged { searchHeight = with(density) { it.height.toDp() } }
                .statusBarsPadding().padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TonalCircleButton(ZIcons.Back, "Back", onClick = onBack)
            Spacer(Modifier.width(10.dp))
            // The search field: a full pill.
            GlassSurface(modifier = Modifier.weight(1f)) {
                Row(Modifier.heightIn(min = 44.dp).padding(horizontal = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                    ZIcon(ZIcons.Search, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.width(12.dp))
                    Box(Modifier.weight(1f)) {
                        if (query.isEmpty()) Text("Search sessions", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
                        BasicTextField(
                            query,
                            { query = it },
                            singleLine = true,
                            textStyle = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface),
                            cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                            modifier = Modifier.fillMaxWidth().focusRequester(focus),
                        )
                    }
                }
            }
        }
    }
}
