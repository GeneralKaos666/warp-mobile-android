package dev.warp.symlinktest

import android.os.Bundle
import android.system.Os
import android.util.Log
import androidx.appcompat.app.AppCompatActivity
import java.io.File

class MainActivity : AppCompatActivity() {

    companion object {
        private const val TAG = "SymlinkExec"
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Thread {
            runTest()
        }.start()
    }

    private fun runTest() {
        try {
            val nativeLibDir = applicationInfo.nativeLibraryDir
            val soPath = "$nativeLibDir/libhello_exec.so"
            val binDir = File(filesDir, "usr/bin")
            binDir.mkdirs()
            val symlinkPath = File(binDir, "hello_exec")

            // Remove old symlink if exists
            if (symlinkPath.exists() || isSymlink(symlinkPath)) {
                symlinkPath.delete()
            }

            Log.i(TAG, "nativeLibDir=$nativeLibDir so_exists=${File(soPath).exists()}")

            // Create symlink via android.system.Os
            try {
                Os.symlink(soPath, symlinkPath.absolutePath)
                Log.i(TAG, "symlink_created: ${symlinkPath.absolutePath} -> $soPath")
            } catch (e: Exception) {
                Log.e(TAG, "symlink_failed errno=${e.message}")
                reportResult(-1, "", "symlink_failed:${e.message}")
                return
            }

            // Execute via symlink path
            try {
                val process = Runtime.getRuntime().exec(symlinkPath.absolutePath)
                val exitCode = process.waitFor()
                val stdout = process.inputStream.bufferedReader().readText().trim()
                val stderr = process.errorStream.bufferedReader().readText().trim()
                Log.i(TAG, "result_exit=$exitCode stdout_token=$stdout stderr=$stderr")
                reportResult(exitCode, stdout, null)
            } catch (e: Exception) {
                Log.e(TAG, "exec_failed errno=${e.message}")
                reportResult(-2, "", "exec_failed:${e.message}")
            }

        } catch (e: Exception) {
            Log.e(TAG, "unexpected_error: ${e.message}")
            reportResult(-3, "", "unexpected:${e.message}")
        }
    }

    private fun reportResult(exitCode: Int, stdoutToken: String, errno: String?) {
        val passed = exitCode == 42 && stdoutToken == "SYMLINK_EXEC_TOKEN_OK"
        val errnoStr = errno ?: "null"
        Log.i(TAG, "RESULT: result_exit=$exitCode stdout_token=$stdoutToken errno=$errnoStr passed=$passed")
    }

    private fun isSymlink(file: File): Boolean {
        return try {
            file.canonicalPath != file.absolutePath
        } catch (e: Exception) {
            false
        }
    }
}
