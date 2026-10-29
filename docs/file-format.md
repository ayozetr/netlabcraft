# The `.pcraft` lab file

UTF-8 JSON:

```json
{"format": "packetcraft", "version": 1, "topology": {"devices": [...], "links": [...], "next_id": 7, "notes": "..."}}
```

- Each device: `id`, `kind` (`router`, `switch`, `hub`, `pc`, `laptop`, `server`, `printer`), `model` (catalogue id such as `Switch-48`; empty in older files means the kind's default), `name`, `x`, `y`, `ports`, `gateway`, `routes`, `vlans`, `startup`.
- Each port: `name`, `mac`, `shutdown`, `description`, `ip` (`"10.0.0.1/24"`), `switchport` (`mode`, `access_vlan`, `native_vlan`, `allowed` as `"1-10,20"`), `subifs`.
- Each link: `a` and `b`, each `{"device": id, "port": index}`.
- `startup` holds the saved configuration as console lines.
- `annotations`: notes placed on the workspace, each `{"id", "x", "y", "text"}`.

Readers accept older versions, refuse newer ones, cap the size (`MAX_BYTES`) and sanitize everything (duplicate ids/names, dangling cables, non-finite positions). Dynamic state (ARP, MAC tables, events) is not saved.
