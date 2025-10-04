package org.chromium.net;

import java.nio.ByteBuffer;

public abstract class UploadDataProvider {
    public abstract long getLength();
    public abstract void read(UploadDataSink uploadDataSink, ByteBuffer byteBuffer);
    public abstract void rewind(UploadDataSink uploadDataSink);
}
