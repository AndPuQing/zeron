package sh.zeron.android.design

import android.app.Activity
import android.content.ContextWrapper
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.LocalRippleConfiguration
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat

/** Zeron Light / Zeron Dark (crates/theme builtins): cool neutrals, violet accent. */
private val ZeronLight = lightColorScheme(
    primary = Color(0xFF5B43E8),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFE5E0FF),
    onPrimaryContainer = Color(0xFF1E0E6B),
    inversePrimary = Color(0xFF8B7CF6),
    secondary = Color(0xFF5E5A71),
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFE4E1F0),
    onSecondaryContainer = Color(0xFF1C1A29),
    tertiary = Color(0xFF0F766E),
    onTertiary = Color.White,
    tertiaryContainer = Color(0xFFCCF2EC),
    onTertiaryContainer = Color(0xFF00201D),
    background = Color(0xFFF3F3F5),
    onBackground = Color(0xFF27272C),
    surface = Color(0xFFF3F3F5),
    onSurface = Color(0xFF27272C),
    surfaceVariant = Color(0xFFE7E7EB),
    onSurfaceVariant = Color(0xFF62626A),
    surfaceTint = Color(0xFF5B43E8),
    surfaceBright = Color(0xFFFFFFFF),
    surfaceDim = Color(0xFFE2E2E6),
    surfaceContainerLowest = Color(0xFFFFFFFF),
    surfaceContainerLow = Color(0xFFFAFAFB),
    surfaceContainer = Color(0xFFEEEEF1),
    surfaceContainerHigh = Color(0xFFE8E8EC),
    surfaceContainerHighest = Color(0xFFE2E2E7),
    outline = Color(0xFF97979F),
    outlineVariant = Color(0xFFE2E2E6),
    error = Color(0xFFDC2626),
    onError = Color.White,
    errorContainer = Color(0xFFFDE2E2),
    onErrorContainer = Color(0xFF5F1111),
    inverseSurface = Color(0xFF27272C),
    inverseOnSurface = Color(0xFFF3F3F5),
    scrim = Color.Black,
)

private val ZeronDark = darkColorScheme(
    primary = Color(0xFF8B7CF6),
    onPrimary = Color(0xFF1A0F5C),
    primaryContainer = Color(0xFF2F2670),
    onPrimaryContainer = Color(0xFFE5E0FF),
    inversePrimary = Color(0xFF5B43E8),
    secondary = Color(0xFFC8C3DC),
    onSecondary = Color(0xFF302D40),
    secondaryContainer = Color(0xFF26242F),
    onSecondaryContainer = Color(0xFFE4E1F0),
    tertiary = Color(0xFF5EEAD4),
    onTertiary = Color(0xFF003731),
    tertiaryContainer = Color(0xFF0C3A35),
    onTertiaryContainer = Color(0xFFCCF2EC),
    background = Color(0xFF060606),
    onBackground = Color(0xFFE8E8EA),
    surface = Color(0xFF060606),
    onSurface = Color(0xFFE8E8EA),
    surfaceVariant = Color(0xFF1C1C20),
    onSurfaceVariant = Color(0xFFA9A9AE),
    surfaceTint = Color(0xFF8B7CF6),
    surfaceBright = Color(0xFF26262A),
    surfaceDim = Color(0xFF060606),
    surfaceContainerLowest = Color(0xFF000000),
    surfaceContainerLow = Color(0xFF0E0E10),
    surfaceContainer = Color(0xFF131316),
    surfaceContainerHigh = Color(0xFF1A1A1E),
    surfaceContainerHighest = Color(0xFF222227),
    outline = Color(0xFF6B6B72),
    outlineVariant = Color(0xFF1E1E22),
    error = Color(0xFFF87171),
    onError = Color(0xFF450A0A),
    errorContainer = Color(0xFF3B1212),
    onErrorContainer = Color(0xFFFDE2E2),
    inverseSurface = Color(0xFFE8E8EA),
    inverseOnSurface = Color(0xFF111113),
    scrim = Color.Black,
)

/** Geist, from the same asset bytes the transcript measures. */
val Geist: FontFamily by lazy {
    FontFamily(
    Font("Geist.ttf", LocalAssets.manager, FontWeight.Normal),
    Font("Geist-Medium.ttf", LocalAssets.manager, FontWeight.Medium),
    Font("Geist-SemiBold.ttf", LocalAssets.manager, FontWeight.SemiBold),
    Font("Geist-Bold.ttf", LocalAssets.manager, FontWeight.Bold),
    Font("Geist-Italic.ttf", LocalAssets.manager, FontWeight.Normal, FontStyle.Italic),
    )
}

val GeistMono: FontFamily by lazy {
    FontFamily(
    Font("GeistMono.ttf", LocalAssets.manager, FontWeight.Normal),
    Font("GeistMono-Medium.ttf", LocalAssets.manager, FontWeight.Medium),
    Font("GeistMono-SemiBold.ttf", LocalAssets.manager, FontWeight.SemiBold),
    )
}

/** App-wide AssetManager for font families declared at top level. */
object LocalAssets {
    lateinit var manager: android.content.res.AssetManager
}

/** UIKit shell type sizes, independent of Material's display and label scales. */
private fun zeronTypography(): Typography {
    fun type(size: Float, line: Float, weight: FontWeight = FontWeight.Normal) = TextStyle(
        fontFamily = Geist, fontWeight = weight, fontSize = size.sp, lineHeight = line.sp,
        letterSpacing = 0.sp, platformStyle = PlatformTextStyle(includeFontPadding = false),
    )
    return Typography(
        displayLarge = type(34f, 41f, FontWeight.SemiBold),
        displayMedium = type(34f, 41f, FontWeight.SemiBold),
        displaySmall = type(30f, 36f, FontWeight.SemiBold),
        headlineLarge = type(30f, 36f, FontWeight.SemiBold),
        headlineMedium = type(28f, 34f, FontWeight.SemiBold),
        headlineSmall = type(22f, 28f, FontWeight.SemiBold),
        titleLarge = type(20f, 25f, FontWeight.SemiBold),
        titleMedium = type(16.5f, 22f, FontWeight.Medium),
        titleSmall = type(15f, 20f, FontWeight.Medium),
        bodyLarge = type(17f, 24f),
        bodyMedium = type(15f, 21f),
        bodySmall = type(13f, 18f),
        labelLarge = type(15f, 20f, FontWeight.Medium),
        labelMedium = type(13f, 18f, FontWeight.Medium),
        labelSmall = type(11f, 14f, FontWeight.Medium),
        displayLargeEmphasized = type(34f, 41f, FontWeight.SemiBold),
        displayMediumEmphasized = type(34f, 41f, FontWeight.SemiBold),
        displaySmallEmphasized = type(30f, 36f, FontWeight.SemiBold),
        headlineLargeEmphasized = type(30f, 36f, FontWeight.SemiBold),
        headlineMediumEmphasized = type(28f, 34f, FontWeight.SemiBold),
        headlineSmallEmphasized = type(22f, 28f, FontWeight.SemiBold),
        titleLargeEmphasized = type(20f, 25f, FontWeight.SemiBold),
        titleMediumEmphasized = type(16.5f, 22f, FontWeight.SemiBold),
        titleSmallEmphasized = type(15f, 20f, FontWeight.SemiBold),
        bodyLargeEmphasized = type(17f, 24f, FontWeight.SemiBold),
        bodyMediumEmphasized = type(15f, 21f, FontWeight.Medium),
        bodySmallEmphasized = type(13f, 18f, FontWeight.Medium),
        labelLargeEmphasized = type(15f, 20f, FontWeight.SemiBold),
        labelMediumEmphasized = type(13f, 18f, FontWeight.SemiBold),
        labelSmallEmphasized = type(11f, 14f, FontWeight.SemiBold),
    )
}

enum class ThemeMode { System, Light, Dark }

data class Appearance(val mode: ThemeMode = ThemeMode.System)

/** Whether the resolved theme is dark (the transcript palette follows it). */
val LocalDarkTheme = staticCompositionLocalOf { false }

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun ZeronTheme(appearance: Appearance = Appearance(), content: @Composable () -> Unit) {
    val dark = when (appearance.mode) {
        ThemeMode.System -> isSystemInDarkTheme()
        ThemeMode.Light -> false
        ThemeMode.Dark -> true
    }
    val scheme = if (dark) ZeronDark else ZeronLight
    val typography = remember { zeronTypography() }
    val view = LocalView.current
    val activity = remember(view) {
        generateSequence(view.context) { (it as? ContextWrapper)?.baseContext }.filterIsInstance<Activity>().firstOrNull()
    }
    SideEffect {
        activity?.window?.let { window ->
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !dark
                isAppearanceLightNavigationBars = !dark
            }
        }
    }
    CompositionLocalProvider(LocalDarkTheme provides dark, LocalRippleConfiguration provides null) {
        MaterialTheme(
            colorScheme = scheme,
            motionScheme = MotionScheme.standard(),
            typography = typography,
            content = content,
        )
    }
}
