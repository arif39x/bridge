import bridge_rs

def map_type(sql_type):
    """Map SQL data types to Python types."""
    mapping = {
        "uuid": "uuid.UUID",
        "text": "str",
        "varchar": "str",
        "timestamp": "datetime",
        "integer": "int",
    }
    return mapping.get(sql_type.lower(), "Any")

async def reflect_table(table_name: str) -> str:
    """Reflect a database table and return its Python class definition."""
    columns = bridge_rs.reflect_table(table_name)
    
    class_name = "".join(x.capitalize() for x in table_name.split("_"))
    if class_name.endswith("s"):
        class_name = class_name[:-1]
        
    lines = [
        f"class {class_name}(BaseModel):",
        f"    table = \"{table_name}\""
    ]
    
    for col in columns:
        py_type = map_type(col.data_type)
        lines.append(f"    {col.name}: {py_type}")
        
    return "\n".join(lines)
