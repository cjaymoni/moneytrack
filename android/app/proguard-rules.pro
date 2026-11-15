# JNA reaches the native library by reflection, so the shrinker cannot see the references
# and removes the classes that carry them. These three lines are what keeps a release build
# able to open the ledger at all.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.mt_ffi.** { *; }
