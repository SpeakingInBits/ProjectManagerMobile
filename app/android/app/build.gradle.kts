// Packages the Rust library (built by cargo-ndk) into an APK / Play Store app bundle. The app has
// no Java/Kotlin code: Android's NativeActivity loads libproject_app.so directly.
import javax.inject.Inject
import org.gradle.process.ExecOperations

plugins {
    id("com.android.application")
}

val rustAbis = listOf("arm64-v8a", "x86_64")
val minSdkVersion = 26
val compileSdkVersion = 36

// Release builds are signed with this keystore when it's given (the release workflow passes it from
// repository secrets); otherwise they're left unsigned.
val releaseKeystore = providers.environmentVariable("ANDROID_KEYSTORE_PATH").orNull

android {
    namespace = "com.speakinginbits.projectmanager"
    compileSdk = compileSdkVersion

    defaultConfig {
        applicationId = "com.speakinginbits.projectmanager"
        // API 26 (Android 8.0) is the minimum Slint's Android backend supports.
        minSdk = minSdkVersion
        targetSdk = compileSdkVersion
        // The release workflow passes -PversionCode and -PversionName for each release.
        versionCode = providers.gradleProperty("versionCode").map(String::toInt).getOrElse(1)
        versionName = providers.gradleProperty("versionName").getOrElse("0.1.0")
        ndk { abiFilters += rustAbis }
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = providers.environmentVariable("ANDROID_KEYSTORE_PASSWORD").get()
                keyAlias = providers.environmentVariable("ANDROID_KEY_ALIAS").get()
                // Keystores made with keytool's defaults use the store password for the key too.
                keyPassword = providers.environmentVariable("ANDROID_KEY_PASSWORD")
                    .orElse(providers.environmentVariable("ANDROID_KEYSTORE_PASSWORD")).get()
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (releaseKeystore != null) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }
}

/** Builds libproject_app.so for each ABI with cargo-ndk, into [outputDir]. */
abstract class CargoNdkBuild : DefaultTask() {
    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @get:Input
    abstract val abis: ListProperty<String>

    @get:Input
    abstract val minSdk: Property<Int>

    @get:Input
    abstract val androidJar: Property<String>

    @get:Internal
    abstract val workspaceDir: DirectoryProperty

    @get:Inject
    abstract val execOps: ExecOperations

    init {
        // Cargo tracks the Rust sources itself and is fast when nothing changed.
        outputs.upToDateWhen { false }
    }

    @TaskAction
    fun build() {
        val args = mutableListOf("cargo", "ndk")
        abis.get().forEach { args += listOf("-t", it) }
        // Always an optimized build: debug Rust is far slower and the APK runs on devices.
        args += listOf(
            "-P", minSdk.get().toString(),
            "-o", outputDir.get().asFile.absolutePath,
            "build", "--release", "-p", "project-app", "--lib",
        )
        execOps.exec {
            workingDir(workspaceDir.get().asFile)
            // cargo-ndk sets ANDROID_PLATFORM to the min SDK, which would make Slint's build
            // script look for that platform's android.jar; point it at the compile SDK's jar.
            environment("ANDROID_JAR", androidJar.get())
            commandLine(args)
        }
    }
}

val cargoBuild = tasks.register<CargoNdkBuild>("cargoBuild") {
    abis.set(rustAbis)
    minSdk.set(minSdkVersion)
    workspaceDir.set(rootProject.layout.projectDirectory.dir("../.."))
    androidJar.set(
        androidComponents.sdkComponents.sdkDirectory.map {
            it.file("platforms/android-$compileSdkVersion/android.jar").asFile.absolutePath
        },
    )
}

androidComponents {
    onVariants { variant ->
        variant.sources.jniLibs?.addGeneratedSourceDirectory(cargoBuild, CargoNdkBuild::outputDir)
    }
}
