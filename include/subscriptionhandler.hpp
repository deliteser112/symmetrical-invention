/*
 * ******************************************************************************
 * Copyright (c) 2018 Robert Bosch GmbH.
 *
 * All rights reserved. This program and the accompanying materials
 * are made available under the terms of the Eclipse Public License v2.0
 * which accompanies this distribution, and is available at
 * https://www.eclipse.org/org/documents/epl-2.0/index.php
 *
 *  Contributors:
 *      Robert Bosch GmbH - initial API and functionality
 * *****************************************************************************
 */
#ifndef __SUBSCRIPTIONHANDLER_H__
#define __SUBSCRIPTIONHANDLER_H__

#include <mutex>
#include <queue>
#include <unordered_map>
#include "visconf.hpp"
#include <string>
#include <thread>
#include <memory>

#include <jsoncons/json.hpp>

class accesschecker;
class Authenticator;
class vssdatabase;
class wschannel;
class WsServer;
class ILogger;

// Subscription ID: Client ID
typedef std::unordered_map<uint32_t, uint32_t> subscriptions_t;

// Subscription UUID
typedef std::string uuid_t;

class subscriptionhandler {
 private:
  std::shared_ptr<ILogger> logger;
  std::unordered_map<uuid_t, subscriptions_t> subscribeHandle;
  WsServer* server;
  Authenticator* validator;
  accesschecker* checkAccess;
  std::mutex subMutex;
  std::thread subThread;
  bool threadRun;
  std::queue<std::pair<uint32_t, jsoncons::json>> buffer;

 public:
  subscriptionhandler(std::shared_ptr<ILogger> loggerUtil,
                      WsServer* wserver,
                      Authenticator* authenticate,
                      accesschecker* checkAccess);
  ~subscriptionhandler();

  uint32_t subscribe(wschannel& channel, vssdatabase* db,
                     uint32_t channelID, std::string path);
  int unsubscribe(uint32_t subscribeID);
  int unsubscribeAll(uint32_t connectionID);
  int updateByUUID(std::string signalUUID, jsoncons::json value);
  int updateByPath(std::string path, jsoncons::json value);
  WsServer* getServer();
  int startThread();
  int stopThread();
  bool isThreadRunning();
  void* subThreadRunner();
};
#endif
