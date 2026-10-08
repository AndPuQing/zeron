# JNA loads native methods and inspects structures/callbacks through reflection.
-keep class com.sun.jna.** { *; }
-keepattributes *Annotation*,InnerClasses,EnclosingMethod
# JNA's desktop AWT helpers are unavailable and unused on Android.
-dontwarn java.awt.**

# UniFFI registers these Kotlin objects' native methods by their original names.
-keep class uniffi.zeron_core.UniffiLib { *; }
-keep class uniffi.zeron_core.IntegrityCheckingUniffiLib { *; }

# FieldOrder refers to fields by name; JNA also constructs Structure subclasses.
-keep class uniffi.zeron_core.** extends com.sun.jna.Structure { *; }
-keep class uniffi.zeron_core.** implements com.sun.jna.Callback { *; }
