/*
 * ******************************************************************************
 * Copyright (c) 2020 Robert Bosch GmbH.
 *
 * All rights reserved. This program and the accompanying materials
 * are made available under the terms of the Eclipse Public License v2.0
 * which accompanies this distribution, and is available at
 * https://www.eclipse.org/org/documents/epl-2.0/index.php
 *
 *  Contributors:
 *      Robert Bosch GmbH
 * *****************************************************************************
 */

#include "JsonResponses.hpp"
#include "VSSPath.hpp"
#include "VSSRequestValidator.hpp"
#include "VssCommandProcessor.hpp"
#include "exception.hpp"

#include <boost/algorithm/string.hpp>
#include <boost/algorithm/string/join.hpp>
#include <vector>
#include "ILogger.hpp"
#include "IVssDatabase.hpp"

/** Implements the Websocket get request accroding to GEN2, with GEN1 backwards
 * compatibility **/
std::string VssCommandProcessor::processGet2(WsChannel &channel,
                                             jsoncons::json &request) {
  std::string pathStr= request["path"].as_string();
  VSSPath path = VSSPath::fromVSS(pathStr);

  try {
    requestValidator->validateGet(request);
  } catch (jsoncons::jsonschema::schema_error &e) {
    std::string msg = std::string(e.what());
    boost::algorithm::trim(msg);
    logger->Log(LogLevel::ERROR, msg);
    return JsonResponses::malFormedRequest(
        requestValidator->tryExtractRequestId(request), "get",
        string("Schema error: ") + msg);
  } catch (std::exception &e) {
    std::string msg = std::string(e.what());
    boost::algorithm::trim(msg);
    logger->Log(LogLevel::ERROR, "Unhandled error: " + msg);
    return JsonResponses::malFormedRequest(
        requestValidator->tryExtractRequestId(request), "get",
        string("Unhandled error: ") + e.what());
  }

  string requestId = request["requestId"].as_string();

  logger->Log(LogLevel::VERBOSE, "Get request with id " + requestId +
                                     " for path: " + path.to_string());

  jsoncons::json answer;
  jsoncons::json valueArray = jsoncons::json::array();
  std::vector<std::string> noPermissionPaths;

  try {
    list<VSSPath> vssPaths = database->getLeafPaths(path);
    for (const auto &vssPath : vssPaths) {
      // check Read access here.
      if (!accessValidator_->checkReadAccess(channel, vssPath)) {
        noPermissionPaths.push_back(vssPath.to_string());
      } else {
        // TODO: This will add the "last"  timestamp, changing behavior from previous
        //"timestamp of the get request" approach 
        //Both are not very helpful when querying multiple values.
        //This will be fixed once https://github.com/eclipse/kuksa.val/issues/158
        //is implemented, as VISS2 will allow attaching individual timestamps to
        //individual data
        jsoncons::json signal = database->getSignal(vssPath);
        answer["timestamp"] = signal["timestamp"].as<string>();
        signal.erase("timestamp");
        valueArray.push_back(signal);
      }
    }
    if (vssPaths.size() < 1) {
      return JsonResponses::pathNotFound(requestId, "get", pathStr);
    }
    if (vssPaths.size() == noPermissionPaths.size()) {
      stringstream msg;
      msg << "No read access to " << pathStr;
      logger->Log(LogLevel::ERROR, msg.str());
      return JsonResponses::noAccess(requestId, "get", msg.str());
    }
    if (vssPaths.size() == 1) {
      answer["path"] = (pathStr);
      answer.insert_or_assign("value", valueArray[0][pathStr]);
    } else {
      answer["value"] = valueArray;
    }
    if (noPermissionPaths.size() > 0) {
      stringstream msg;
      msg << "No read access to [ "
          << boost::algorithm::join(noPermissionPaths, ",") << " ]";
      answer["warning"] = std::string(msg.str());
    }
  } catch (std::exception &e) {
    logger->Log(LogLevel::ERROR, "Unhandled error: " + string(e.what()));
    return JsonResponses::malFormedRequest(
        requestId, "get", string("Unhandled error: ") + e.what());
  }

  answer["action"] = "get";
  answer["requestId"] = requestId;
  stringstream ss;
  ss << pretty_print(answer);
  return ss.str();
}
