class BridgeError(Exception):
    """Base exception for all Bridge errors."""
    pass

class ConnectionError(BridgeError):
    """Raised when database connection fails."""
    pass

class QueryError(BridgeError):
    """Raised when a database query fails."""
    pass

class NotFoundError(BridgeError, KeyError):
    """Raised when a requested resource is not found in the database."""
    pass

class ConstraintError(BridgeError):
    """Raised when a database constraint is violated."""
    pass

class ValidationError(BridgeError, ValueError):
    """Raised when data fails validation before database interaction."""
    pass

class HookAbortedError(BridgeError):
    """Raised when a pre-save/delete hook aborts the operation."""
    pass

class DatabaseError(BridgeError):
    """Raised when the database engine returns an error."""
    pass

class ProjectionError(BridgeError, AttributeError):
    """Raised when accessing an unselected field in a partial model."""
    pass

class CompositeKeyError(BridgeError):
    """Raised when composite primary key operations fail."""
    pass
