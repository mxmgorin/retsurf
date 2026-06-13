# Release is not minified (minifyEnabled false), so these are mostly belt-and-
# suspenders. SDL reaches its Java glue and our activity by name via JNI/reflection;
# keep them if shrinking is ever turned on.
-keep class org.libsdl.app.** { *; }
-keep class com.retsurf.** { *; }
