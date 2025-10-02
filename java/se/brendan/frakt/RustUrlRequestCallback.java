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
    private final long handlerId;

    public RustUrlRequestCallback(long handlerId) {
        this.handlerId = handlerId;
    }

    @Override
    public native void onRedirectReceived(
        UrlRequest request,
        UrlResponseInfo info,
        String newLocationUrl
    ) throws Exception;

    @Override
    public native void onResponseStarted(
        UrlRequest request,
        UrlResponseInfo info
    ) throws Exception;

    @Override
    public native void onReadCompleted(
        UrlRequest request,
        UrlResponseInfo info,
        ByteBuffer byteBuffer
    ) throws Exception;

    @Override
    public native void onSucceeded(
        UrlRequest request,
        UrlResponseInfo info
    );

    @Override
    public native void onFailed(
        UrlRequest request,
        UrlResponseInfo info,
        CronetException error
    );

    public long getHandlerId() {
        return handlerId;
    }
}
