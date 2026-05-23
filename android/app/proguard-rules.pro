# Keep OkHttp
-dontwarn okhttp3.**
-dontwarn okio.**
-keep class okhttp3.** { *; }
-keep class okio.** { *; }

# Keep silero-vad (ONNX runtime + native libs)
-dontwarn com.konovalov.vad.**
-keep class com.konovalov.vad.** { *; }
-dontwarn ai.onnxruntime.**
-keep class ai.onnxruntime.** { *; }
