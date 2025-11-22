# Examples for developing Daemon Console
Here are some examples based on some Python codes, for [I](https://github.com/Mooling0602) master Python better at present.

## Downstream plugin react info events
> Based on [MCDReforged](https://github.com/MCDReforged/MCDReforged).

Part of the code:

```python
...

def on_info(server: PluginServerInterface, info: Info):
    if info.content == "test":
        server.logger.info("ok.")

...
```

**Implementation target**

Allow downstream project (e.g. demo/main.rs in this project) to response specific input events like this.