package se.brendan.frakt;

import org.chromium.net.UrlRequest;
import org.chromium.net.UrlResponseInfo;
import org.chromium.net.CronetException;
import java.nio.ByteBuffer;

/**
 * Minimal UrlRequest.Callback implementation that delegates to Rust via JNI.
 * This class is compiled at build time and embedded in the binary.
 *
 * All methods are native and implemented in Rust.
 */
public class RustUrlRequestCallback extends UrlRequest.Callback {
    // static {
    //     System.loadLibrary("frakt");
    // }

    private final long handlerId;

    public RustUrlRequestCallback(long handlerId) {
        this.handlerId = handlerId;
    }

    @Override
    public void onRedirectReceived(
        UrlRequest request,
        UrlResponseInfo info,
        String newLocationUrl
    ) throws Exception {
        nativeOnRedirectReceived(request, info, newLocationUrl);
    }

    @Override
    public void onResponseStarted(
        UrlRequest request,
        UrlResponseInfo info
    ) throws Exception {
        nativeOnResponseStarted(request, info);
    }

    @Override
    public void onReadCompleted(
        UrlRequest request,
        UrlResponseInfo info,
        ByteBuffer byteBuffer
    ) throws Exception {
        nativeOnReadCompleted(request, info, byteBuffer);
    }

    @Override
    public void onSucceeded(
        UrlRequest request,
        UrlResponseInfo info
    ) {
        nativeOnSucceeded(request, info);
    }

    @Override
    public void onFailed(
        UrlRequest request,
        UrlResponseInfo info,
        CronetException error
    ) {
        nativeOnFailed(request, info, error);
    }

    private native void nativeOnRedirectReceived(
        UrlRequest request,
        UrlResponseInfo info,
        String newLocationUrl
    ) throws Exception;

    private native void nativeOnResponseStarted(
        UrlRequest request,
        UrlResponseInfo info
    ) throws Exception;

    private native void nativeOnReadCompleted(
        UrlRequest request,
        UrlResponseInfo info,
        ByteBuffer byteBuffer
    ) throws Exception;

    private native void nativeOnSucceeded(
        UrlRequest request,
        UrlResponseInfo info
    );

    private native void nativeOnFailed(
        UrlRequest request,
        UrlResponseInfo info,
        CronetException error
    );

    public long getHandlerId() {
        return handlerId;
    }
}
