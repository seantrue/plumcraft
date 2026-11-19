// PlumCraft's Kotlin kit, stage N0(b): the JNI wrapper as a plain JVM module, so its tests run
// on the host JVM against the host-built library (no device). The Android library module joins
// in N5; the Kotlin source here uses no Android API so it moves as it is.
pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}
dependencyResolutionManagement {
    repositories { mavenCentral() }
}
rootProject.name = "plumcraft-native-android"
include(":plumcraft")
