from typing import Any, Type
from factory import Factory, declarations

class BridgeFactory(Factory):
    """Base factory class for Bridge models."""
    class Meta:
        abstract = True

    @classmethod
    async def _create(cls, model_class: Type, *args, **kwargs) -> Any:
        """Create an instance of the model and save it to the database."""
        # This requires a session. We assume it's passed or available.
        session = kwargs.pop('session', None)
        if not session:
             # Fallback to model.create which uses a one-off session/tx if none provided
             return await model_class.create(**kwargs)
        
        return await model_class.create(tx=session, **kwargs)

    @classmethod
    def create_batch(cls, size: int, **kwargs) -> Any:
        # factory_boy's create_batch is usually sync.
        # For async ORMs, it's better to use a custom async batch creator.
        raise NotImplementedError("Use create_batch_async for Bridge factories")

    @classmethod
    async def create_batch_async(cls, size: int, **kwargs) -> list:
        return [await cls.create(**kwargs) for _ in range(size)]
