# Keep the JNI native methods (called from BarkPipeline)
-keep class com.mrsobakin.bark.BarkPipeline { *; }

# Keep kotlinx.coroutines (used for async)
-keep class kotlinx.coroutines.** { *; }
