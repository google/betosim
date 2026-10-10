/*
 * Copyright (C) 2024 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

package android.test.wifi.nsd;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

import android.content.Context;
import android.net.nsd.NsdManager;
import android.net.nsd.NsdServiceInfo;
import android.net.wifi.WifiManager;
import android.util.Log;
import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PrintWriter;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

/** Helper class for NsdManager. */
public final class NsdHelper {
  private final NsdManager nsdManager;
  private final WifiManager wifi;
  private WifiManager.MulticastLock multicastLock;

  private String serviceName = "WifiNsdTest";
  private static final String SERVICE_TYPE = "_wifi_nsd_test._tcp.";
  private static final String TAG = "WifiTest-NsdInstrumentationTest";
  private static final String PING = "Hello";
  private static final String PONG = "World";

  public NsdHelper(Context context, String testId) {
    this.wifi = context.getSystemService(WifiManager.class);
    // Previously unregistered services may still be discoverable.
    // Use unique service name to prevent the discovery of unregistered services.
    this.serviceName += "-" + testId;
    this.multicastLock = this.wifi.createMulticastLock("multicastLock");
    this.nsdManager = context.getSystemService(NsdManager.class);
  }

  private void await(CountDownLatch latch) throws InterruptedException {
    assertTrue(latch.await(60, TimeUnit.SECONDS));
  }

  public void serviceTest() throws InterruptedException, IOException {
    ServerSocket serverSocket = new ServerSocket(0);
    RegistrationListener listener = registerService(serverSocket.getLocalPort());
    await(listener.serviceRegistered);

    Socket clientSocket = serverSocket.accept();

    // Wait for ping message.
    BufferedReader in = new BufferedReader(new InputStreamReader(clientSocket.getInputStream()));
    String msg = in.readLine();
    assertEquals(PING, msg);

    // Send pong message.
    PrintWriter out = new PrintWriter(clientSocket.getOutputStream(), true);
    out.println(PONG);

    serverSocket.close();
    clientSocket.close();

    unregisterService(listener);
    await(listener.serviceUnregistered);
  }

  private static class RegistrationListener implements NsdManager.RegistrationListener {
    CountDownLatch serviceRegistered;
    CountDownLatch serviceUnregistered;

    RegistrationListener() {
      serviceRegistered = new CountDownLatch(1);
      serviceUnregistered = new CountDownLatch(1);
    }

    @Override
    public void onServiceRegistered(NsdServiceInfo serviceInfo) {
      Log.d(TAG, "Service registered. NsdServiceInfo:" + serviceInfo);
      serviceRegistered.countDown();
    }

    @Override
    public void onServiceUnregistered(NsdServiceInfo serviceInfo) {
      Log.d(TAG, "Service unregistered");
      serviceUnregistered.countDown();
    }

    @Override
    public void onRegistrationFailed(NsdServiceInfo serviceInfo, int errorCode) {
      fail("Registration failed");
    }

    @Override
    public void onUnregistrationFailed(NsdServiceInfo serviceInfo, int errorCode) {
      fail("Unregistration failed");
    }
  }

  private RegistrationListener registerService(int port) {
    RegistrationListener listener = new RegistrationListener();
    NsdServiceInfo serviceInfo = new NsdServiceInfo();
    serviceInfo.setPort(port);
    serviceInfo.setServiceName(serviceName);
    serviceInfo.setServiceType(SERVICE_TYPE);

    nsdManager.registerService(serviceInfo, NsdManager.PROTOCOL_DNS_SD, listener);
    return listener;
  }

  private CountDownLatch unregisterService(RegistrationListener listener) {
    nsdManager.unregisterService(listener);
    return listener.serviceUnregistered;
  }

  public void discoverTest() throws InterruptedException, IOException {
    DiscoveryListener listener = discoverServices();
    await(listener.serviceFound);
    ServiceInfoCallback callback = resolveServices(listener.service);
    await(callback.serviceResolved);
    CountDownLatch discoveryStopped = stopDiscovery(listener);
    await(discoveryStopped);

    // Set up connection.
    NsdServiceInfo service = callback.service;
    Socket clientSocket = new Socket(service.getHost(), service.getPort());

    // Send ping message.
    PrintWriter out = new PrintWriter(clientSocket.getOutputStream(), true);
    out.println(PING);

    // Wait for pong message.
    BufferedReader in = new BufferedReader(new InputStreamReader(clientSocket.getInputStream()));
    String msg = in.readLine();
    assertEquals(PONG, msg);
    clientSocket.close();
  }

  private static class DiscoveryListener implements NsdManager.DiscoveryListener {
    CountDownLatch serviceFound;
    CountDownLatch discoveryStopped;
    NsdServiceInfo service;

    private String serviceName;

    DiscoveryListener(String serviceName) {
      serviceFound = new CountDownLatch(1);
      discoveryStopped = new CountDownLatch(1);
      this.serviceName = serviceName;
    }

    @Override
    public void onDiscoveryStarted(String regType) {}

    @Override
    public void onServiceFound(NsdServiceInfo serviceInfo) {
      if (serviceInfo.getServiceType().equals(SERVICE_TYPE)
          && serviceInfo.getServiceName().equals(serviceName)) {
        service = serviceInfo;
        serviceFound.countDown();
      }
    }

    @Override
    public void onServiceLost(NsdServiceInfo nsdServiceInfo) {}

    @Override
    public void onDiscoveryStopped(String serviceType) {
      discoveryStopped.countDown();
    }

    @Override
    public void onStartDiscoveryFailed(String serviceType, int errorCode) {
      fail("Failed to start discovery");
    }

    @Override
    public void onStopDiscoveryFailed(String serviceType, int errorCode) {
      fail("Failed to stop discovery");
    }
  }

  private DiscoveryListener discoverServices() {
    DiscoveryListener discoveryListener = new DiscoveryListener(serviceName);

    multicastLock.setReferenceCounted(true);
    multicastLock.acquire();

    nsdManager.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, discoveryListener);

    return discoveryListener;
  }

  private ServiceInfoCallback resolveServices(NsdServiceInfo service) {
    ServiceInfoCallback callback = new ServiceInfoCallback(serviceName);
    nsdManager.registerServiceInfoCallback(service, Executors.newSingleThreadExecutor(), callback);

    return callback;
  }

  private static class ServiceInfoCallback implements NsdManager.ServiceInfoCallback {
    CountDownLatch serviceResolved;
    NsdServiceInfo service;

    private String serviceName;

    ServiceInfoCallback(String serviceName) {
      serviceResolved = new CountDownLatch(1);
      this.serviceName = serviceName;
    }

    @Override
    public void onServiceUpdated(NsdServiceInfo serviceInfo) {
      if (serviceInfo.getServiceName().equals(serviceName)) {
        service = serviceInfo;
        serviceResolved.countDown();
      }
    }

    @Override
    public void onServiceInfoCallbackRegistrationFailed(int errorCode) {
      fail("Callback registration failed");
    }

    @Override
    public void onServiceInfoCallbackUnregistered() {}

    @Override
    public void onServiceLost() {}
  }

  private CountDownLatch stopDiscovery(DiscoveryListener listener) {
    nsdManager.stopServiceDiscovery(listener);
    if (multicastLock.isHeld()) {
      multicastLock.release(); // release after browsing
    }
    return listener.discoveryStopped;
  }
}
