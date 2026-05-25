package com.mrsobakin.bark

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Bundle

class PermissionBridgeActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            BarkKeyboardService.permissionResult.trySend(true)
            finish()
            return
        }

        requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 0)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        val granted = grantResults.isNotEmpty() &&
                grantResults[0] == PackageManager.PERMISSION_GRANTED

        BarkKeyboardService.permissionResult.trySend(granted)
        finish()
    }
}
