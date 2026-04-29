package dev.warp.symlinktest

import android.os.Bundle
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.util.Log
import androidx.appcompat.app.AppCompatActivity
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException

class MainActivity : AppCompatActivity() {

    companion object {
        private const val TAG = "SymlinkExec"
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Thread { runTest(); finish() }.start()
    }

    private fun runTest() {
        try {
            val nativeLibDir = applicationInfo.nativeLibraryDir
            val soPath = "$nativeLibDir/libhello_exec.so"
            val binDir = File(filesDir, "usr/bin")
            binDir.mkdirs()

            Log.i(TAG, "nativeLibDir=$nativeLibDir so_exists=${File(soPath).exists()}")

            // --- Negative control: copy binary to filesDir and exec directly ---
            // API 29+ W^X policy blocks execve() on writable app-data files.
            // Expect FAILURE (IOException wrapping EACCES) on SDK >= 29.
            val copyPath = File(binDir, "hello_exec_copy")
            var negativeControlFailed = false
            var negativeErrno = "none"
            var negativeErrnoName = "none"
            try {
                copyPath.delete()
                FileInputStream(soPath).use { inp ->
                    FileOutputStream(copyPath).use { out -> inp.copyTo(out) }
                }
                copyPath.setExecutable(true, false)
                val proc = Runtime.getRuntime().exec(copyPath.absolutePath)
                val ncStdout = Thread { proc.inputStream.bufferedReader().readText() }
                val ncStderr = Thread { proc.errorStream.bufferedReader().readText() }
                ncStdout.start(); ncStderr.start()
                val exitCode = proc.waitFor()
                ncStdout.join(); ncStderr.join()
                // Restriction not enforced — exec succeeded
                Log.w(TAG, "negative_control_unexpectedly_passed exit=$exitCode")
                negativeControlFailed = false
                negativeErrno = "exec_succeeded_exit=$exitCode"
                negativeErrnoName = "succeeded"
            } catch (e: ErrnoException) {
                // Direct ErrnoException path (e.g. from Os.execv)
                negativeControlFailed = true
                negativeErrno = "ErrnoException(${e.errno}):${e.message}"
                negativeErrnoName = OsConstants.errnoName(e.errno) ?: "E${e.errno}"
                Log.i(TAG, "negative_control_denied errno=${e.errno} msg=${e.message}")
            } catch (e: IOException) {
                // Runtime.exec wraps OS errors as IOException; this is the expected path on API 29+
                // Parse numeric errno from message (e.g. "error=13") for structured logging.
                negativeControlFailed = true
                negativeErrno = "IOException:${e.message}"
                val errnoInt = Regex("error=(\\d+)").find(e.message ?: "")?.groupValues?.get(1)?.toIntOrNull()
                negativeErrnoName = if (errnoInt != null) OsConstants.errnoName(errnoInt) ?: "E$errnoInt" else "IOException"
                Log.i(TAG, "negative_control_denied IOException msg=${e.message}")
            } catch (e: Exception) {
                negativeControlFailed = true
                negativeErrno = "${e.javaClass.simpleName}:${e.message}"
                negativeErrnoName = e.javaClass.simpleName
                Log.i(TAG, "negative_control_denied ${e.javaClass.simpleName} msg=${e.message}")
            }

            // --- Symlink test: exec via symlink pointing into nativeLibraryDir ---
            val symlinkPath = File(binDir, "hello_exec")
            if (symlinkPath.exists() || isSymlink(symlinkPath)) symlinkPath.delete()

            var symlinkPassed = false
            var symlinkErrno = "none"
            var symlinkExit = -99
            var symlinkToken = ""

            try {
                // Os.symlink throws ErrnoException on failure with proper errno
                Os.symlink(soPath, symlinkPath.absolutePath)
                Log.i(TAG, "symlink_created: ${symlinkPath.absolutePath} -> $soPath")
            } catch (e: ErrnoException) {
                symlinkErrno = "symlink_ErrnoException(${e.errno}):${e.message}"
                Log.e(TAG, symlinkErrno)
                logFinalResult(negativeControlFailed, negativeErrno, negativeErrnoName, false, symlinkErrno, -1, "")
                return
            }

            try {
                val proc = Runtime.getRuntime().exec(symlinkPath.absolutePath)
                // Drain streams on separate threads before waitFor() to avoid deadlock
                // when stdout/stderr pipe buffer fills (common in release builds).
                var stdoutText = ""
                var stderrText = ""
                val stdoutThread = Thread { stdoutText = proc.inputStream.bufferedReader().readText().trim() }
                val stderrThread = Thread { stderrText = proc.errorStream.bufferedReader().readText().trim() }
                stdoutThread.start(); stderrThread.start()
                symlinkExit = proc.waitFor()
                stdoutThread.join(5000); stderrThread.join(5000)
                symlinkToken = stdoutText
                val stderr = stderrText
                Log.i(TAG, "symlink_exec result_exit=$symlinkExit stdout_token=$symlinkToken stderr=$stderr")
                symlinkPassed = symlinkExit == 42 && symlinkToken == "SYMLINK_EXEC_TOKEN_OK"
            } catch (e: ErrnoException) {
                symlinkErrno = "exec_ErrnoException(${e.errno}):${e.message}"
                Log.e(TAG, "symlink_exec_denied errno=${e.errno} msg=${e.message}")
            } catch (e: IOException) {
                symlinkErrno = "exec_IOException:${e.message}"
                Log.e(TAG, "symlink_exec_failed IOException msg=${e.message}")
            } catch (e: Exception) {
                symlinkErrno = "exec_${e.javaClass.simpleName}:${e.message}"
                Log.e(TAG, "symlink_exec_failed ${e.javaClass.simpleName} msg=${e.message}")
            }

            logFinalResult(negativeControlFailed, negativeErrno, negativeErrnoName, symlinkPassed, symlinkErrno, symlinkExit, symlinkToken)

        } catch (e: Exception) {
            Log.e(TAG, "unexpected_error: ${e.message}")
            Log.i(TAG, "RESULT: negative_control_failed=false NEGATIVE_ERRNO_BEGINunexpectedNEGATIVE_ERRNO_END negative_errno_name=unexpected symlink_passed=false symlink_errno=unexpected result_exit=-99 stdout_token=")
        }
    }

    private fun logFinalResult(
        negativeControlFailed: Boolean,
        negativeErrno: String,
        negativeErrnoName: String,
        symlinkPassed: Boolean,
        symlinkErrno: String,
        symlinkExit: Int,
        symlinkToken: String
    ) {
        // Use sentinel delimiters for negative_errno so the full message (with spaces) is parseable.
        Log.i(
            TAG,
            "RESULT: negative_control_failed=$negativeControlFailed" +
            " NEGATIVE_ERRNO_BEGIN${negativeErrno}NEGATIVE_ERRNO_END" +
            " negative_errno_name=$negativeErrnoName" +
            " symlink_passed=$symlinkPassed symlink_errno=$symlinkErrno" +
            " result_exit=$symlinkExit stdout_token=$symlinkToken"
        )
    }

    private fun isSymlink(file: File): Boolean {
        return try { file.canonicalPath != file.absolutePath } catch (e: Exception) { false }
    }
}
