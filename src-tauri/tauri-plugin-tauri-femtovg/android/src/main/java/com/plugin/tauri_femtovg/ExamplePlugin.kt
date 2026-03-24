package com.plugin.tauri_femtovg

import android.app.Activity
import android.util.Log
import android.view.View
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import app.tauri.plugin.Invoke

@InvokeArg
class PingArgs {
    var value: String? = null
}

@TauriPlugin
class ExamplePlugin(private val activity: Activity) : Plugin(activity) {
    private val implementation = Example()

    private var surfaceView: android.view.SurfaceView? = null

    companion object {
        var surface: android.view.Surface? = null
    }

    override fun load(webView: WebView) {
        super.load(webView)

        // Make WebView transparent and ensure hardware acceleration
        webView.setBackgroundColor(0) // Color.TRANSPARENT
        webView.setLayerType(View.LAYER_TYPE_HARDWARE, null)

        // Create and inject SurfaceView
        surfaceView = android.view.SurfaceView(activity)
        surfaceView?.setZOrderOnTop(false) // Ensure it's behind the window

        surfaceView?.holder?.addCallback(object : android.view.SurfaceHolder.Callback {
            override fun surfaceCreated(holder: android.view.SurfaceHolder) {
                surface = holder.surface
                Log.d("ExamplePlugin", "WGPU Surface created")
            }

            override fun surfaceChanged(
                holder: android.view.SurfaceHolder,
                format: Int,
                width: Int,
                height: Int
            ) {
                Log.d("ExamplePlugin", "WGPU Surface changed: ${width}x${height}")
            }

            override fun surfaceDestroyed(holder: android.view.SurfaceHolder) {
                surface = null
                Log.d("ExamplePlugin", "WGPU Surface destroyed")
            }
        })

        // Add SurfaceView behind WebView
        val parent = webView.parent as? android.view.ViewGroup
        if (parent != null) {
            Log.d("ExamplePlugin", "WebView parent has ${parent.childCount} children before adding SurfaceView")

            // Index 0 puts it behind everything else
            parent.addView(
                surfaceView, 0, android.view.ViewGroup.LayoutParams(
                    android.view.ViewGroup.LayoutParams.MATCH_PARENT,
                    android.view.ViewGroup.LayoutParams.MATCH_PARENT
                )
            )
        }
    }

    @Command
    fun ping(invoke: Invoke) {
        val args = invoke.parseArgs(PingArgs::class.java)

        val ret = JSObject()
        ret.put("value", implementation.pong(args.value ?: "default value :("))
        invoke.resolve(ret)
    }
}
