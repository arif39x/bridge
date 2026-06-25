import asyncio

from bridge import connect, NotFoundError, User, Post


async def main():
    # Use SQLite for testing
    await connect("sqlite://:memory:")

    # For this prototype, the tables must exist.
    # We would normally run: CREATE TABLE users (id UUID PRIMARY KEY, username TEXT, email TEXT, created_at TIMESTAMP, updated_at TIMESTAMP);
    print("Testing User creation...")
    try:
        Miku = await User.create(username="Miku", email="bluehairbluetie@gmail.com")
        print(f"Miku created with ID: {Miku.id}")

        print("Testing User retrieval...")
        found = await User.find_one(id=Miku.id)
        print(f"Found user: {found.username}")

        print("Testing related post loading...")
        posts = await Miku.load_related(Post)
        print(f"Loaded {len(posts)} posts for {found.username}")

        print("Testing error handling for nonexistent user...")
        try:
            await User.find_one(id="00000000-0000-0000-0000-000000000000")
        except NotFoundError as e:
            print(f"Caught expected error: {e}")
    except Exception as e:
        print(f"Error during test: {e}")
        print("Note: In a real test, you'd need the SQLite tables to exist.")


if __name__ == "__main__":
    asyncio.run(main())
