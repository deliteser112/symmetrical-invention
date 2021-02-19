#! /usr/bin/env python

########################################################################
# Copyright (c) 2020 Robert Bosch GmbH
#
# This program and the accompanying materials are made
# available under the terms of the Eclipse Public License 2.0
# which is available at https://www.eclipse.org/legal/epl-2.0/
#
# SPDX-License-Identifier: EPL-2.0
########################################################################


import os, sys, configparser, signal
import json
import time

from dapr.clients import DaprClient

scriptDir= os.path.dirname(os.path.realpath(__file__))
sys.path.append(os.path.join(scriptDir, "../common/"))
from clientComm import VSSClientComm

class Kuksa_Client():

    # Constructor
    def __init__(self, config):
        print("Init kuksa client...")
        if "kuksa_val" not in config:
            print("kuksa_val section missing from configuration, exiting")
            sys.exit(-1)
        provider_config=config['kuksa_val']
        self.client = VSSClientComm(provider_config)
        self.client.start()
        self.token = provider_config.get('token', "token.json")
        self.client.authorize(self.token)
        
    def shutdown(self):
        self.client.stopComm()

    def subscribe(self, path, callback):
        print("subscribe " + path)
        res = self.client.subscribe(path, callback)
        print(res)

class Dapr_Publisher():
    def __init__(self, config, producer):
        print("Init dapr publisher...")
        if "dapr" not in config:
            print("dapr section missing from configuration, exiting")
            sys.exit(-1)
        
        self.producer = producer
        dapr_config=config['dapr']
        self.topic=dapr_config.get('topic')

        self.producer.subscribe(self.topic, self.publisher)

        self.daprClient = DaprClient()

    def publisher(self, kuksa_message):
        print("KUKSA: " + kuksa_message)
        jsonMsg = json.loads(kuksa_message) 
        req_data = {
            'id': jsonMsg["subscriptionId"],
            'timestamp': jsonMsg["timestamp"],
            'value': jsonMsg["value"],
            'topic': self.topic
        }

        # Create a typed message with content type and body
        resp = self.daprClient.publish_event(
            pubsub_name='pubsub',
            topic_name=self.topic,
            data=json.dumps(req_data),
            data_content_type='application/json',
        )

        # Print the request
        print(req_data, flush=True)



    def shutdown(self):
        self.producer.shutdown()

        
if __name__ == "__main__":
    config_candidates=['config.ini']
    for candidate in config_candidates:
        if os.path.isfile(candidate):
            configfile=candidate
            break
    if configfile is None:
        print("No configuration file found. Exiting")
        sys.exit(-1)
    config = configparser.ConfigParser()
    config.read(configfile)
    
    client = Dapr_Publisher(config, Kuksa_Client(config))

    def terminationSignalreceived(signalNumber, frame):
        print("Received termination signal. Shutting down")
        client.shutdown()
    signal.signal(signal.SIGINT, terminationSignalreceived)
    signal.signal(signal.SIGQUIT, terminationSignalreceived)
    signal.signal(signal.SIGTERM, terminationSignalreceived)


