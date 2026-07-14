package com.mrsobakin.bark

import org.json.JSONArray
import org.json.JSONObject

const val PREF_AGC = "audio_agc"
const val PREF_VAD = "audio_vad"
const val PREF_POST_PROCESSORS = "text_postprocessors"

sealed interface PostProcessorStep {
    data object Normalize : PostProcessorStep
    data class Regex(val pattern: String, val replacement: String) : PostProcessorStep
}

object PipelineSettings {
    fun decodePostProcessors(raw: String?): MutableList<PostProcessorStep> {
        if (raw == null) return mutableListOf(PostProcessorStep.Normalize)

        return try {
            val array = JSONArray(raw)
            MutableList(array.length()) { index ->
                val item = array.getJSONObject(index)
                when (item.getString("type")) {
                    "normalize" -> PostProcessorStep.Normalize
                    "regex" -> PostProcessorStep.Regex(
                        pattern = item.getString("pattern"),
                        replacement = item.getString("with"),
                    )
                    else -> error("Unknown postprocessor")
                }
            }
        } catch (_: Exception) {
            mutableListOf(PostProcessorStep.Normalize)
        }
    }

    fun encodePostProcessors(steps: List<PostProcessorStep>): String =
        postProcessorsJson(steps).toString()

    fun postProcessorsJson(steps: List<PostProcessorStep>): JSONArray = JSONArray().apply {
        steps.forEach { step ->
            put(
                when (step) {
                    PostProcessorStep.Normalize -> JSONObject().put("type", "normalize")
                    is PostProcessorStep.Regex -> JSONObject().apply {
                        put("type", "regex")
                        put("pattern", step.pattern)
                        put("with", step.replacement)
                    }
                },
            )
        }
    }
}
