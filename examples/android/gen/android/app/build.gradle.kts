plugins {
    id("com.android.application")
    id("rust")
    id("org.jetbrains.kotlin.android")
}

val sourceDirs = listOf("src/main/java", "src/fake/java")

// Log contents of each source directory
sourceDirs.forEach { dirPath ->
    val dir = File(project.projectDir, dirPath)
    println("ASDFFDSA: Checking source dir: $dir")

    if (!dir.exists()) {
        println("ASDFFDSA:   ❌ Directory does not exist.")
    } else {
        val javaFiles = dir.walkTopDown().filter { it.extension == "java" }.toList()
        if (javaFiles.isEmpty()) {
            println("ASDFFDSA:   ⚠️ No .java files found.")
        } else {
            println("ASDFFDSA:   ✅ Found ${javaFiles.size} .java file(s):")
            javaFiles.forEach { println("    - ${it.relativeTo(project.projectDir)}") }
        }
    }
}

android {
    namespace="com.example.android"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.example.android"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"
    }
    sourceSets.getByName("main") {
        // Vulkan validation layers
        val ndkHome = System.getenv("NDK_HOME")
        jniLibs.srcDir("${ndkHome}/sources/third_party/vulkan/src/build-android/jniLibs")
        java.setSrcDirs(sourceDirs)
    }
    buildTypes {
        getByName("debug") {
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
             proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
}

rust {
    rootDirRel = "../../"
}

dependencies {
    implementation("com.google.android.material:material:1.8.0")
    implementation("androidx.core:core-ktx:1.16.0")
}
