package com.inkprint.app

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Indication
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.node.DelegatableNode
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog

// E-ink look: black on white, borders and solid blocks instead of shade, no
// animation of any kind. Electronic paper shows greys as dither and every
// animation frame as a partial refresh that leaves ghosting.
//
// The solid blocks (primary buttons, the chosen tab or option, progress, the
// running status, notices) are black on black-and-white screens and coloured
// everywhere else. The colours are dark and saturated so they survive the
// washed-out palette of colour e-paper (Kaleido) and carry white text.

val Ink = Color.Black
val Paper = Color.White

/** Fills of the solid blocks; text on them is [Paper]. */
data class InkPalette(
    /** Primary buttons, the chosen tab / option / toggle side, progress, steps. */
    val accent: Color,
    /** The running status. */
    val good: Color,
    /** Notices that need attention. */
    val warn: Color,
)

val MonoPalette = InkPalette(accent = Ink, good = Ink, warn = Ink)
val ColorPalette = InkPalette(
    accent = Color(0xFF0B57D0),  // blue
    good = Color(0xFF146C2E),    // green
    warn = Color(0xFFB3261E),    // red
)

val LocalInkPalette = staticCompositionLocalOf { MonoPalette }

/** The current palette's fills. */
val Accent: Color @Composable get() = LocalInkPalette.current.accent
val Good: Color @Composable get() = LocalInkPalette.current.good
val Warn: Color @Composable get() = LocalInkPalette.current.warn

private val InkColors = lightColorScheme(
    primary = Ink, onPrimary = Paper,
    primaryContainer = Paper, onPrimaryContainer = Ink,
    secondary = Ink, onSecondary = Paper,
    secondaryContainer = Paper, onSecondaryContainer = Ink,
    tertiary = Ink, onTertiary = Paper,
    background = Paper, onBackground = Ink,
    surface = Paper, onSurface = Ink,
    surfaceVariant = Paper, onSurfaceVariant = Ink,
    surfaceTint = Paper,
    surfaceContainer = Paper, surfaceContainerHigh = Paper, surfaceContainerHighest = Paper,
    surfaceContainerLow = Paper, surfaceContainerLowest = Paper,
    inverseSurface = Ink, inverseOnSurface = Paper,
    outline = Ink, outlineVariant = Ink,
    error = Ink, onError = Paper,
    scrim = Color.Transparent,
)

private val InkType = Typography(
    bodyLarge = TextStyle(fontSize = 17.sp, lineHeight = 24.sp, color = Ink),
    bodyMedium = TextStyle(fontSize = 16.sp, lineHeight = 22.sp, color = Ink),
    bodySmall = TextStyle(fontSize = 14.sp, lineHeight = 20.sp, color = Ink),
    titleLarge = TextStyle(fontSize = 24.sp, fontWeight = FontWeight.Bold, color = Ink),
    titleMedium = TextStyle(fontSize = 19.sp, fontWeight = FontWeight.Bold, color = Ink),
    labelLarge = TextStyle(fontSize = 17.sp, fontWeight = FontWeight.Bold),
)

/** Draws nothing on press: no ripple, no highlight to animate. */
private object NoIndication : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode = object : Modifier.Node() {}
    override fun hashCode() = 0
    override fun equals(other: Any?) = other === this
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun InkTheme(color: Boolean, content: @Composable () -> Unit) {
    val square = androidx.compose.foundation.shape.RoundedCornerShape(0.dp)
    MaterialTheme(colorScheme = InkColors, typography = InkType, shapes = Shapes(
        extraSmall = square, small = square, medium = square, large = square, extraLarge = square,
    )) {
        CompositionLocalProvider(
            LocalIndication provides NoIndication as Indication,
            LocalRippleConfiguration provides null,
            LocalTextSelectionColors provides TextSelectionColors(handleColor = Ink, backgroundColor = Paper),
            LocalContentColor provides Ink,
            LocalInkPalette provides if (color) ColorPalette else MonoPalette,
        ) {
            Surface(color = Paper, contentColor = Ink, content = content)
        }
    }
}

/** A tap target without any press feedback (e-ink can't show it cleanly). */
fun Modifier.tap(enabled: Boolean = true, onClick: () -> Unit): Modifier =
    this.clickable(enabled = enabled, onClick = onClick)

/** Black filled (primary) or black-outlined button, square, at least 56 dp tall. */
@Composable
fun InkButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    primary: Boolean = false,
    enabled: Boolean = true,
    height: Dp = 56.dp,
) {
    val filled = primary && enabled
    Box(
        modifier = modifier
            .heightIn(min = height)
            .background(if (filled) Accent else Paper)
            // Disabled: a hairline frame and plain text — no grey on e-ink.
            .border(if (enabled) 2.dp else 1.dp, if (filled) Accent else Ink)
            .tap(enabled, onClick)
            .padding(horizontal = 14.dp, vertical = 8.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text,
            color = if (filled) Paper else Ink,
            fontSize = 17.sp,
            fontWeight = if (enabled) FontWeight.Bold else FontWeight.Normal,
            textAlign = TextAlign.Center,
        )
    }
}

/** A small text action (e.g. "Copy"), underlined instead of coloured. */
@Composable
fun InkLink(text: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Text(
        text,
        modifier = modifier.tap(onClick = onClick).padding(horizontal = 6.dp, vertical = 10.dp),
        fontSize = 15.sp,
        fontWeight = FontWeight.Bold,
        style = TextStyle(textDecoration = androidx.compose.ui.text.style.TextDecoration.Underline),
    )
}

/** A thin black frame around a block. */
@Composable
fun InkCard(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = modifier.fillMaxWidth().border(1.5.dp, Ink).padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
        content = content
    )
}

/** Section heading: bold, with a rule underneath. */
@Composable
fun InkHeading(text: String, modifier: Modifier = Modifier) {
    Column(modifier.fillMaxWidth()) {
        Text(text, fontSize = 19.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(4.dp))
        Box(Modifier.fillMaxWidth().height(2.dp).background(Ink))
    }
}

/**
 * A setting with an ON / OFF pair: the chosen side is black. Changes at once,
 * nothing slides.
 */
@Composable
fun InkToggle(title: String, subtitle: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(title, fontSize = 17.sp, fontWeight = FontWeight.Bold)
            if (subtitle != null) Text(subtitle, fontSize = 14.sp, lineHeight = 19.sp)
        }
        Row(Modifier.border(2.dp, Ink)) {
            for ((label, value) in listOf(stringResource(R.string.toggle_off) to false, stringResource(R.string.toggle_on) to true)) {
                val on = checked == value
                Box(
                    Modifier
                        .size(width = 58.dp, height = 48.dp)
                        .background(if (on) Accent else Paper)
                        .tap { if (!on) onChange(value) },
                    contentAlignment = Alignment.Center
                ) {
                    Text(label, color = if (on) Paper else Ink, fontSize = 15.sp, fontWeight = FontWeight.Bold)
                }
            }
        }
    }
}

/** A row of mutually exclusive choices; the chosen one is black. */
@Composable
fun <T> InkSegmented(options: List<Pair<T, String>>, selected: T, onSelect: (T) -> Unit, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().border(2.dp, Ink)) {
        options.forEachIndexed { i, (value, label) ->
            if (i > 0) Box(Modifier.width(2.dp).height(48.dp).background(Ink))
            val on = value == selected
            Box(
                Modifier
                    .weight(1f)
                    .height(48.dp)
                    .background(if (on) Accent else Paper)
                    .tap { onSelect(value) },
                contentAlignment = Alignment.Center
            ) {
                Text(
                    label,
                    color = if (on) Paper else Ink,
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Bold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = 2.dp)
                )
            }
        }
    }
}

/** Bottom tab bar; the current tab is inverted. */
@Composable
fun InkTabBar(tabs: List<String>, selected: Int, onSelect: (Int) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        Box(Modifier.fillMaxWidth().height(2.dp).background(Ink))
        Row(Modifier.fillMaxWidth().height(60.dp)) {
            tabs.forEachIndexed { i, label ->
                val on = i == selected
                Box(
                    Modifier
                        .weight(1f)
                        .fillMaxHeight()
                        .background(if (on) Accent else Paper)
                        .tap { onSelect(i) },
                    contentAlignment = Alignment.Center
                ) {
                    Text(label, color = if (on) Paper else Ink, fontSize = 17.sp, fontWeight = FontWeight.Bold)
                }
            }
        }
    }
}

/** White box with a heavy black frame; no scrim fade, no shadow. */
@Composable
fun InkDialog(
    title: String,
    onDismiss: () -> Unit,
    buttons: @Composable RowScope.() -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    Dialog(onDismissRequest = onDismiss) {
        Column(
            Modifier
                .fillMaxWidth()
                .background(Paper)
                .border(3.dp, Ink)
                .padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Text(title, fontSize = 20.sp, fontWeight = FontWeight.Bold)
            Column(
                Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(10.dp),
                content = content
            )
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp), content = buttons)
        }
    }
}

/** A notice in white on black, for what needs attention (no Wi-Fi, ...). */
@Composable
fun InkNotice(text: String) {
    Text(
        text,
        color = Paper,
        fontSize = 15.sp,
        fontWeight = FontWeight.Bold,
        modifier = Modifier.fillMaxWidth().background(Warn).padding(12.dp)
    )
}

/** A printer address in monospace with a Copy action. */
@Composable
fun CopyableText(text: String) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier.fillMaxWidth().border(1.dp, Ink).padding(start = 10.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            text,
            fontFamily = FontFamily.Monospace,
            fontSize = 14.sp,
            modifier = Modifier.weight(1f).padding(vertical = 8.dp)
        )
        InkLink(
            stringResource(if (copied) R.string.copied else R.string.copy),
            onClick = { clipboard.setText(AnnotatedString(text)); copied = true }
        )
    }
}

/** A numbered step: the number in a black square. */
@Composable
fun Step(number: Int, text: String) {
    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
        Box(
            modifier = Modifier.size(28.dp).background(Accent),
            contentAlignment = Alignment.Center
        ) {
            Text(number.toString(), color = Paper, fontSize = 15.sp, fontWeight = FontWeight.Bold)
        }
        Spacer(Modifier.width(10.dp))
        Text(text, fontSize = 16.sp, lineHeight = 22.sp, modifier = Modifier.weight(1f))
    }
}

/** Commands to type: white on black, tap to copy. */
@Composable
fun CodeBlock(text: String) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(Ink)
            .tap { clipboard.setText(AnnotatedString(text)); copied = true }
            .padding(12.dp)
    ) {
        Text(text, fontFamily = FontFamily.Monospace, fontSize = 13.sp, color = Paper, lineHeight = 18.sp)
        Spacer(Modifier.height(6.dp))
        Text(
            stringResource(if (copied) R.string.copied else R.string.tap_to_copy),
            fontSize = 13.sp,
            color = Paper,
            fontWeight = FontWeight.Bold
        )
    }
}

/**
 * Progress as a black bar in a frame. It only changes when progress does
 * (every few seconds at most): nothing moves in between.
 */
@Composable
fun InkProgress(fraction: Float, modifier: Modifier = Modifier, height: Dp = 14.dp) {
    Box(modifier.fillMaxWidth().height(height).border(1.5.dp, Ink)) {
        Box(Modifier.fillMaxHeight().fillMaxWidth(fraction.coerceIn(0f, 1f)).background(Accent))
    }
}

/** A short label in a frame (file kind: PDF, EPUB, folder). */
@Composable
fun InkTag(text: String, modifier: Modifier = Modifier) {
    Box(
        modifier.border(BorderStroke(1.5.dp, Ink)).widthIn(min = 52.dp).padding(horizontal = 4.dp, vertical = 6.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(text, fontSize = 12.sp, fontWeight = FontWeight.Bold, maxLines = 1)
    }
}
