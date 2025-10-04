package com.google.common.util.concurrent;

public interface ListenableFuture<V> {
    V get() throws Exception;
}
