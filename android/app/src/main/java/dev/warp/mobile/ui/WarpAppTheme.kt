package dev.warp.mobile.ui

import android.app.Activity
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat

/**
 * M7 (iteration 20 — Warp UX scaffold): Material 3 theme tokens for the
 * Compose chrome around the warpui Vulkan terminal pane. Color palette
 * mirrors Warp Desktop's dark default — warm-purple primary on a near-black
 * surface; the accent shows up on the top-bar action icons + drawer
 * selected-row chip + send-button on the prompt composer.
 *
 * On Android 12+ (API 31+) the theme participates in Material You dynamic
 * color when the user has wallpaper-derived colors enabled; otherwise the
 * static dark/light palettes below win.
 */
private val WarpDarkColors = darkColorScheme(
    primary = Color(0xFFC58AF9),         // warm purple — Warp Desktop accent
    onPrimary = Color(0xFF1F0E37),
    primaryContainer = Color(0xFF4A2C7A),
    onPrimaryContainer = Color(0xFFEFDDFF),
    secondary = Color(0xFF7DCAE0),       // cool cyan accent for /remote-control etc
    onSecondary = Color(0xFF002E3A),
    background = Color(0xFF0F0F11),      // near-black; matches the terminal
    onBackground = Color(0xFFE6E6EB),
    surface = Color(0xFF15151A),
    onSurface = Color(0xFFE6E6EB),
    surfaceVariant = Color(0xFF26262E),
    onSurfaceVariant = Color(0xFFB0B0BB),
    outline = Color(0xFF45454F),
    outlineVariant = Color(0xFF2C2C34),
    error = Color(0xFFF2B0B0),
    onError = Color(0xFF5A0000),
)

private val WarpLightColors = lightColorScheme(
    primary = Color(0xFF7B3FE4),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFE9D5FF),
    onPrimaryContainer = Color(0xFF2C0B5E),
    secondary = Color(0xFF008396),
    onSecondary = Color(0xFFFFFFFF),
    background = Color(0xFFFCFCFE),
    onBackground = Color(0xFF1A1A1C),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF1A1A1C),
)

@Composable
fun WarpAppTheme(
    useDarkTheme: Boolean = isSystemInDarkTheme(),
    useDynamicColor: Boolean = true,
    content: @Composable () -> Unit
) {
    val colors = when {
        useDynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val ctx = LocalContext.current
            if (useDarkTheme) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
        }
        useDarkTheme -> WarpDarkColors
        else -> WarpLightColors
    }

    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as Activity).window
            window.statusBarColor = colors.background.toArgb()
            window.navigationBarColor = colors.background.toArgb()
            WindowCompat.getInsetsController(window, view).isAppearanceLightStatusBars =
                !useDarkTheme
        }
    }

    MaterialTheme(
        colorScheme = colors,
        content = content
    )
}
