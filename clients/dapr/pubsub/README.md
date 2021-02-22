# Publish kuksa.val data to dapr runtime

This example utilizes a publisher and a subscriber to show, how kuksa.val works with the dapr pubsub component.

## Pre-requisites

- [Dapr CLI and initialized environment](https://docs.dapr.io/getting-started)
- [Install Python 3.7+](https://www.python.org/downloads/)

All dependencies are listed in [requirements.txt](./requirements.txt):


```bash
pip3 install -r requirements.txt
```

## Run the example

At first, you need to start `kuksa.val` and [testclient](../../vss-testclient) to feed data into `kuksa.val`.
You also need the dapr runtime running on your system.
Then run the following commands to start the dapr subscriper and publisher seperately:

```bash
# 1. Start Subscriber (expose gRPC server receiver on port 50051)
dapr run --app-id python-subscriber --app-protocol grpc --app-port 50051 ./subscriber.py
dapr run --app-id python-publisher --app-protocol grpc --dapr-grpc-port=3500 ./publisher.py
```

## Cleanup

```bash
dapr stop --app-id python-publisher
dapr stop --app-id python-subscriber
```
