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
#ifndef __AUTHENTICATOR_H__
#define __AUTHENTICATOR_H__

#include <memory>
#include <string>

using namespace std;

class WsChannel;
class VssDatabase;
class ILogger;

class Authenticator {
 private:
  string pubkey = "secret";
  string algorithm = "RS256";
  std::shared_ptr<ILogger> logger;

  int validateToken(WsChannel& channel, string authToken);

 public:
  Authenticator(std::shared_ptr<ILogger> loggerUtil, string secretkey, string algorithm);

  int validate(WsChannel &channel, VssDatabase *database,
               string authToken);

  void updatePubKey(string key);
  bool isStillValid(WsChannel &channel);
  void resolvePermissions(WsChannel &channel, VssDatabase *database);
};
#endif
