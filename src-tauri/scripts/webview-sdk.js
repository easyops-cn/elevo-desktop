(function () {
    const global = window;
    const tauriInternals = global.__TAURI_INTERNALS__;
    if (global.elevoMessengerSDK ||
        !tauriInternals)
        return;
    // __WEBVIEW_LABEL__ and __ROOM_ID__ are replaced at runtime by Rust before injection.
    const LABEL = __WEBVIEW_LABEL__;
    const ROOM_ID = __ROOM_ID__;
    const handlers = {};
    // Called by Rust (via webview.eval) to push a message into this webview.
    global.__ElevoMessengerSDK_receive__ = function (channel, data) {
        const fns = handlers[channel] || [];
        fns.forEach(function (fn) {
            try {
                fn(data);
            }
            catch (e) {
                console.error("[ElevoMessengerSDK] handler error", e);
            }
        });
    };
    // Use postMessage transport directly to bypass ipc:// custom protocol,
    // which is blocked by external-page CSP. Uses transformCallback so Tauri
    // can route the response back to the correct promise.
    const tauriInvoke = (cmd, args) => {
        return new Promise(function (resolve, reject) {
            const callback = tauriInternals.transformCallback(resolve, true);
            const error = tauriInternals.transformCallback(reject, true);
            tauriInternals.postMessage({ cmd, callback, error, payload: args });
        });
    };
    /** Send a message to the main window. */
    const sendMessage = (channel, data) => {
        return tauriInvoke("relay_sdk_message", {
            sourceLabel: LABEL,
            roomId: ROOM_ID,
            channel,
            data,
        });
    };
    /**
     * Subscribe to messages pushed from the main window.
     * Returns an unsubscribe function.
     */
    const onMessage = (channel, fn) => {
        if (!handlers[channel])
            handlers[channel] = [];
        handlers[channel].push(fn);
        return function () {
            handlers[channel] = (handlers[channel] || []).filter(function (h) {
                return h !== fn;
            });
        };
    };
    const getLabel = () => {
        return LABEL;
    };
    const getRoomId = () => {
        return ROOM_ID;
    };
    const close = () => {
        return tauriInvoke("close_webview", { label: LABEL });
    };
    const modelContextTools = new Map();
    const modelContext = Object.freeze({
        registerTool(tool) {
            const { name, description, inputSchema } = tool;
            modelContextTools.set(name, tool);
            sendMessage("client_tool_register", { name, description, inputSchema });
        },
        unregisterTool(name) {
            modelContextTools.delete(name);
            sendMessage("client_tool_unregister", { name });
        },
    });
    onMessage("client_tool_execute", async function (data) {
        const { toolName, input, toolCallId } = data;
        const tool = modelContextTools.get(toolName);
        if (!tool) {
            return;
        }
        try {
            const output = await tool.execute(input);
            sendMessage("client_tool_output", {
                toolCallId,
                output,
                success: true,
            });
        }
        catch (e) {
            sendMessage("client_tool_output", {
                toolCallId,
                output: e instanceof Error ? e.message : String(e),
                success: false,
            });
        }
    });
    global.elevoMessengerSDK = Object.freeze({
        sendMessage,
        onMessage,
        getLabel,
        getRoomId,
        close,
        modelContext,
    });
    console.log("[ElevoMessengerSDK] initialized, label:", LABEL, ', roomId:', ROOM_ID);
})();
