(function () {
  if (window.ElevoMessengerSDK) return;

  // __WEBVIEW_LABEL__ and __ROOM_ID__ are replaced at runtime by Rust.
  var LABEL = __WEBVIEW_LABEL__;
  var ROOM_ID = __ROOM_ID__;
  var handlers = {};

  // Called by Rust (via webview.eval) to push a message into this webview.
  window.__ElevoMessengerSDK_receive__ = function (channel, data) {
    var fns = handlers[channel] || [];
    fns.forEach(function (fn) {
      try {
        fn(data);
      } catch (e) {
        console.error('[ElevoMessengerSDK] handler error', e);
      }
    });
  };

  // Use postMessage transport directly to bypass ipc:// custom protocol,
  // which is blocked by external-page CSP. Uses transformCallback so Tauri
  // can route the response back to the correct promise.
  function tauriInvoke(cmd, args) {
    return new Promise(function (resolve, reject) {
      var internals = window.__TAURI_INTERNALS__;
      var callback = internals.transformCallback(resolve, true);
      var error = internals.transformCallback(reject, true);
      internals.postMessage({ cmd: cmd, callback: callback, error: error, payload: args });
    });
  }

  window.ElevoMessengerSDK = {
    /** Send a message to the main window. */
    sendMessage: function (channel, data) {
      return tauriInvoke('relay_sdk_message', {
        sourceLabel: LABEL,
        roomId: ROOM_ID,
        channel: channel,
        data: data,
      });
    },

    /**
     * Subscribe to messages pushed from the main window.
     * Returns an unsubscribe function.
     */
    onMessage: function (channel, fn) {
      if (!handlers[channel]) handlers[channel] = [];
      handlers[channel].push(fn);
      return function () {
        handlers[channel] = (handlers[channel] || []).filter(function (h) {
          return h !== fn;
        });
      };
    },

    getLabel: function () {
      return LABEL;
    },

    getRoomId: function () {
      return ROOM_ID;
    },

    close: function () {
      return tauriInvoke('close_webview', { label: LABEL });
    },
  };

  console.log('[ElevoMessengerSDK] initialized, label:', LABEL, ', roomId:', ROOM_ID);
})();
