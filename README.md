# Dispenser

Automatically spawn and destroy a tf2 server on a schedule

## Usage

- Copy `config.sample.toml` to `config.toml` and edit accordingly
- Start `dispenser config.toml` as a system service

When the configured start schedule is reached it will create a new cloud server, update the dyndns (optional)
and install a tf2 server.
This server is then destroyed when the stop schedule is reached.

As a failsafe against unexpected costs or destroying the wrong server, this program will not spawn any server
if it already detects a running one, and it will only destroy a server that was created by the program.

This does mean that if the program is (re-)started while a server is already active, the program will not
start and destroy any server because it can't be sure it should control the running server.
You'll need to manually destroy the existing server in that case.

You can overwrite this behavior by setting `manage_existing = true` in your `config.toml`,  

## Manual usage

Instead of managing the servers on a schedule with a background server you can also manually manage the server.

### List running servers

```shell
dispenser config.toml list
```

### Start a new server

```shell
dispenser config.toml start
```

Note that this will not start an additional server is a server is already running

### Stop a running server

```shell
dispenser config.toml stop
```

## TODO

- [x] don't blindly kill server if there are players connected
- [ ] kill the server earlier if everyone disconnected
- [x] digital ocean backend
- [ ] more backends?