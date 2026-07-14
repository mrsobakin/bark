package com.mrsobakin.bark

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.os.SystemClock
import android.util.AttributeSet
import android.view.View
import com.google.android.material.color.MaterialColors
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.ln
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.sin

class AudioReactiveBlobView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : View(context, attrs) {

    private val haloPath = Path()
    private val corePath = Path()
    private val pointsX = FloatArray(POINTS)
    private val pointsY = FloatArray(POINTS)
    private val indicatorColor = MaterialColors.getColor(
        this,
        com.google.android.material.R.attr.colorOnSurface,
    )
    private val haloPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
        color = indicatorColor
        alpha = 72
    }
    private val corePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
        color = indicatorColor
    }
    private val ringPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
        color = indicatorColor
    }

    @Volatile
    private var targetLevel = 0f
    private var displayedLevel = 0f
    private var phase = 0f
    private var morph = 0f
    private var heldEnergy = 0f
    private var transcribing = false
    private var running = false
    private var lastFrameMs = 0L

    init {
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    fun setLevel(rms: Float) {
        val db = if (rms > 0f) 20f * (ln(rms.toDouble()) / ln(10.0)).toFloat() else -80f
        targetLevel = ((db + 52f) / 42f).coerceIn(0f, 1f)
        if (running) postInvalidateOnAnimation()
    }

    fun start() {
        transcribing = false
        morph = 0f
        running = true
        lastFrameMs = SystemClock.uptimeMillis()
        postInvalidateOnAnimation()
    }

    fun startTranscribing() {
        heldEnergy = displayedLevel
        transcribing = true
        running = true
        lastFrameMs = SystemClock.uptimeMillis()
        postInvalidateOnAnimation()
    }

    fun stop() {
        running = false
        transcribing = false
        morph = 0f
        heldEnergy = 0f
        targetLevel = 0f
        displayedLevel = 0f
        invalidate()
    }

    override fun onDetachedFromWindow() {
        running = false
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)

        val now = SystemClock.uptimeMillis()
        val dt = if (lastFrameMs == 0L) 0f else ((now - lastFrameMs) / 1000f).coerceIn(0f, 0.05f)
        lastFrameMs = now

        if (!transcribing) {
            val tau = if (targetLevel > displayedLevel) 0.07f else 0.22f
            val smoothing = if (dt == 0f) 1f else 1f - exp(-dt / tau)
            displayedLevel += (targetLevel - displayedLevel) * smoothing
            heldEnergy = displayedLevel
        }

        val morphTarget = if (transcribing) 1f else 0f
        val morphStep = if (dt == 0f) 1f else dt / MORPH_SECONDS
        morph += (morphTarget - morph).coerceIn(-morphStep, morphStep)

        val opacityProgress = (morph / FIRST_OVERSHOOT_FRACTION).coerceAtMost(1f)
        val opacityMorph = 1f - (1f - opacityProgress) * (1f - opacityProgress)
        val geometryMorph = easeOutElastic(morph)
        if (running) {
            val recordingSpeed = 2.4f + heldEnergy * heldEnergy * 8.0f
            phase += dt * lerp(recordingSpeed, TRANSCRIBING_SPEED, opacityMorph)
        }

        val cx = width / 2f
        val cy = height / 2f
        val base = min(width, height) * 0.222f
        val energy = heldEnergy
        val ringRadius = base * 0.675f
        val recordingHaloRadius = base * (1.035f + energy * 1.005f)
        val recordingCoreRadius = base * (0.78f + energy * 0.74f)
        val recordingHaloDeformation = energy * 0.075f
        val recordingCoreDeformation = 0.0175f + energy * 0.075f
        val ringDeformation = 0.105f + sin(phase * 0.37f) * 0.018f

        buildBlob(
            haloPath,
            cx,
            cy,
            recordingHaloRadius,
            recordingHaloDeformation,
            phase * 0.72f + 0.9f,
        )
        buildBlob(
            corePath,
            cx,
            cy,
            lerp(recordingCoreRadius, ringRadius, geometryMorph),
            lerp(recordingCoreDeformation, ringDeformation, geometryMorph),
            phase,
        )

        val remainingFill = 1f - opacityMorph
        val layerAlpha = energy * MAX_LAYER_ALPHA
        haloPaint.alpha = (layerAlpha * remainingFill * remainingFill).toInt()
        corePaint.alpha = (layerAlpha * remainingFill).toInt()
        canvas.drawPath(haloPath, haloPaint)
        canvas.drawPath(corePath, corePaint)

        ringPaint.alpha = (opacityMorph * RING_ALPHA).toInt()
        ringPaint.strokeWidth = resources.displayMetrics.density *
            lerp(2f, 5f + sin(phase * 0.8f) * 0.5f, opacityMorph)
        canvas.drawPath(corePath, ringPaint)

        if (running && isAttachedToWindow) postInvalidateOnAnimation()
    }

    private fun lerp(start: Float, end: Float, amount: Float): Float =
        start + (end - start) * amount

    private fun easeOutElastic(value: Float): Float {
        if (value <= 0f) return 0f
        if (value >= 1f) return 1f
        val c4 = 2f * PI.toFloat() / 3f
        return 2f.pow(-13f * value) * sin((value * 10f - 0.75f) * c4) + 1f
    }

    private fun buildBlob(
        path: Path,
        cx: Float,
        cy: Float,
        radius: Float,
        deformation: Float,
        offset: Float,
    ) {
        path.reset()
        for (point in 0 until POINTS) {
            val angle = point * TWO_PI / POINTS
            val wave = sin(angle * 3f + offset) * 0.47f +
                sin(angle * 5f - offset * 1.31f) * 0.24f +
                cos(angle * 2f + offset * 0.63f) * 0.12f +
                sin(angle * 8f - offset * 2.1f) * 0.17f
            val r = radius * (1f + deformation * wave)
            pointsX[point] = cx + cos(angle) * r
            pointsY[point] = cy + sin(angle) * r
        }

        path.moveTo(pointsX[0], pointsY[0])
        for (point in 0 until POINTS) {
            val previous = (point + POINTS - 1) % POINTS
            val next = (point + 1) % POINTS
            val afterNext = (point + 2) % POINTS
            val control1X = pointsX[point] + (pointsX[next] - pointsX[previous]) * SPLINE_SCALE
            val control1Y = pointsY[point] + (pointsY[next] - pointsY[previous]) * SPLINE_SCALE
            val control2X = pointsX[next] - (pointsX[afterNext] - pointsX[point]) * SPLINE_SCALE
            val control2Y = pointsY[next] - (pointsY[afterNext] - pointsY[point]) * SPLINE_SCALE
            path.cubicTo(
                control1X,
                control1Y,
                control2X,
                control2Y,
                pointsX[next],
                pointsY[next],
            )
        }
        path.close()
    }

    companion object {
        const val TRANSITION_PEAK_MS = 92L

        private const val POINTS = 40
        private const val MORPH_SECONDS = 0.7f
        private const val FIRST_OVERSHOOT_FRACTION = 0.1306f
        private const val TRANSCRIBING_SPEED = 9.0f
        private const val MAX_LAYER_ALPHA = 49f
        private const val RING_ALPHA = 170f
        private const val SPLINE_SCALE = 0.14f
        private const val TWO_PI = (PI * 2.0).toFloat()
    }
}
