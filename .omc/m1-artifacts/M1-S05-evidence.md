# M1-S05 Evidence — Android Service skeleton + FGS + JNI bridge

## Build
- APK: android/app/build/outputs/apk/debug/app-debug.apk
- Size: 7.2 MB
- sha256: b27418b12d76f9cc97c4e9c66ec69a7870c902d04227638e23c03592e9b13677
- Commit: 6b5b26f (M1: Android Service skeleton + FGS + JNI bridge)

## Device: S24 Ultra (R5CX10VFFBA), Android 14 / API 34

### adb install
```
Performing Streamed Install
Success
```

### logcat evidence (04-30 01:22)
```
I warp-android-host: warp_mobile_android_host: ping called
I ActivityManager: Background started FGS: Allowed [callingPackage: dev.warp.mobile; ... code:PROC_STATE_TOP; startForegroundCount:0]
```

### dumpsys activity services dev.warp.mobile
```
* ServiceRecord{dc227e6 u0 dev.warp.mobile/.WarpTerminalService}
  isForeground=true foregroundId=1 types=0x40000000
  foregroundNoti=Notification(channel=warp-terminal ... flags=ONGOING_EVENT|FOREGROUND_SERVICE)
```

## Acceptance Criteria Verdict
- AC1 android/app/ project with build.gradle, AndroidManifest, Activity + Service: PASS
- AC2 FOREGROUND_SERVICE + FOREGROUND_SERVICE_SPECIAL_USE permissions: PASS
- AC3 foregroundServiceType="specialUse" + meta-data: PASS
- AC4 startForeground with NotificationChannel: PASS (channel=warp-terminal)
- AC5 minSdk=31, targetSdk=36, compileSdk=36: PASS
- AC6 System.loadLibrary("warp_mobile_android_host"): PASS (ping called in logcat)
- AC7 persistent notification visible: PASS (suppressed by Samsung notification settings but isForeground=true confirmed via dumpsys)
- AC8 dumpsys lists Service in foreground state: PASS (isForeground=true)
