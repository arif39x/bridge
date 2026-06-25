from typing import Any, List, Optional, Set, Type

from fastapi import APIRouter, Depends, HTTPException

from ..core.base import BaseModel


def generate_router(
    model_class: Type[BaseModel],
    prefix: Optional[str] = None,
    tags: Optional[List[str]] = None,
) -> APIRouter:

    router = APIRouter(
        prefix=prefix or f"/{model_class.table}", tags=tags or [model_class.table]
    )

    @router.get("/", response_model=List[dict])
    async def list_items(limit: int = 100, offset: int = 0):
        items = await model_class.query().limit(limit).fetch()
        return [item.to_dict() for item in items]

    @router.get("/{item_id}", response_model=dict)
    async def get_item(item_id: str):
        try:
            item = await model_class.find_one(id=item_id)
            return item.to_dict()
        except Exception:
            raise HTTPException(
                status_code=404, detail=f"{model_class.__name__} not found"
            )

    @router.post("/", response_model=dict, status_code=201)
    async def create_item(data: dict):
        try:
            item = await model_class.create(**data)
            return item.to_dict()
        except Exception as e:
            raise HTTPException(status_code=400, detail=str(e))

    return router
