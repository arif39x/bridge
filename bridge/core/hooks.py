import functools

from ..common import HookAbortedError


def hook_decorator(hook_point: str):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(cls, *args, **kwargs):
            if not hasattr(cls, "_hooks"):
                cls._hooks = {}
            if hook_point not in cls._hooks:
                cls._hooks[hook_point] = []
            cls._hooks[hook_point].append(func)
            return func

        return wrapper

    return decorator


async def dispatch_hooks(cls, hook_point: str, instance):
    # Run registered hooks for a specific point and model class
    hooks = cls._get_hooks()
    if hook_point not in hooks:
        return True

    for hook in hooks[hook_point]:
        result = await hook(instance)
        if hook_point.startswith("before") and result is False:
            raise HookAbortedError(f"{hook_point} hook cancelled the operation")
    return True
