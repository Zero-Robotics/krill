# Tests for Krill Python SDK

import asyncio
import json
import os
import socket

# Add parent directory to path to import krill module
import sys
import tempfile
import threading
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

import krill


class TestKrillClient(unittest.TestCase):
    """Tests for synchronous KrillClient."""

    def setUp(self):
        """Create a temporary Unix socket for testing."""
        self.temp_dir = tempfile.mkdtemp()
        self.socket_path = os.path.join(self.temp_dir, "test-krill.sock")
        self.server_sock = None
        self.server_thread = None
        self.received_messages = []

    def tearDown(self):
        """Clean up resources."""
        if self.server_sock:
            self.server_sock.close()
        if self.server_thread:
            self.server_thread.join(timeout=1)
        try:
            os.unlink(self.socket_path)
        except FileNotFoundError:
            pass
        try:
            os.rmdir(self.temp_dir)
        except OSError:
            pass

    def start_mock_server(self, num_messages=1):
        """Start a mock Unix socket server that accepts connections."""
        self.server_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server_sock.bind(self.socket_path)
        self.server_sock.listen(1)

        def server_loop():
            conn, _ = self.server_sock.accept()
            for _ in range(num_messages):
                data = conn.recv(4096)
                if not data:
                    break
                self.received_messages.append(data.decode("utf-8"))
            conn.close()

        self.server_thread = threading.Thread(target=server_loop, daemon=True)
        self.server_thread.start()
        # Give server time to start
        import time

        time.sleep(0.1)

    def test_connection_to_nonexistent_socket_raises_error(self):
        """Test that connecting to non-existent socket raises ConnectionError."""
        with self.assertRaises(krill.ConnectionError) as ctx:
            krill.KrillClient("test-service", "/nonexistent/path.sock")
        self.assertIn("Failed to connect", str(ctx.exception))

    def test_heartbeat_sends_correct_json(self):
        """Test that heartbeat() sends correctly formatted JSON."""
        self.start_mock_server()

        client = krill.KrillClient("my-service", self.socket_path)
        client.heartbeat()
        client.close()

        self.server_thread.join(timeout=1)

        self.assertEqual(len(self.received_messages), 1)
        message = json.loads(self.received_messages[0])

        self.assertEqual(message["type"], "heartbeat")
        self.assertEqual(message["service"], "my-service")
        self.assertEqual(message["status"], "healthy")
        self.assertEqual(message["metadata"], {})

    def test_heartbeat_with_metadata(self):
        """Test heartbeat_with_metadata includes metadata."""
        self.start_mock_server()

        client = krill.KrillClient("vision", self.socket_path)
        client.heartbeat_with_metadata({"fps": "30", "latency_ms": "10"})
        client.close()

        self.server_thread.join(timeout=1)

        message = json.loads(self.received_messages[0])
        self.assertEqual(message["metadata"]["fps"], "30")
        self.assertEqual(message["metadata"]["latency_ms"], "10")

    def test_report_degraded(self):
        """Test report_degraded sends degraded status with reason."""
        self.start_mock_server()

        client = krill.KrillClient("camera", self.socket_path)
        client.report_degraded("high temperature")
        client.close()

        self.server_thread.join(timeout=1)

        message = json.loads(self.received_messages[0])
        self.assertEqual(message["status"], "degraded")
        self.assertEqual(message["metadata"]["reason"], "high temperature")

    def test_report_healthy(self):
        """Test report_healthy sends healthy status."""
        self.start_mock_server()

        client = krill.KrillClient("sensor", self.socket_path)
        client.report_healthy()
        client.close()

        self.server_thread.join(timeout=1)

        message = json.loads(self.received_messages[0])
        self.assertEqual(message["status"], "healthy")
        self.assertEqual(message["metadata"], {})

    def test_context_manager_closes_connection(self):
        """Test that using client as context manager closes connection."""
        self.start_mock_server()

        with krill.KrillClient("test", self.socket_path) as client:
            client.heartbeat()

        # Client should be closed after exiting context
        with self.assertRaises(krill.SendError):
            client.heartbeat()

    def test_multiple_heartbeats(self):
        """Test sending multiple heartbeats on same connection."""
        self.start_mock_server(num_messages=3)

        client = krill.KrillClient("lidar", self.socket_path)
        client.heartbeat()
        client.heartbeat_with_metadata({"scan": "1"})
        client.report_healthy()
        client.close()

        self.server_thread.join(timeout=1)

        # Messages may arrive in multiple chunks
        all_data = "".join(self.received_messages)
        lines = all_data.strip().split("\n")
        self.assertEqual(len(lines), 3)


class TestAsyncKrillClient(unittest.TestCase):
    """Tests for asynchronous AsyncKrillClient."""

    def setUp(self):
        """Create a temporary Unix socket for testing."""
        self.temp_dir = tempfile.mkdtemp()
        self.socket_path = os.path.join(self.temp_dir, "test-krill-async.sock")
        self.received_messages = []

    def tearDown(self):
        """Clean up resources."""
        try:
            os.unlink(self.socket_path)
        except FileNotFoundError:
            pass
        try:
            os.rmdir(self.temp_dir)
        except OSError:
            pass

    async def mock_server(self, num_clients=1):
        """Async mock server."""

        async def handle_client(reader, writer):
            while True:
                data = await reader.read(4096)
                if not data:
                    break
                self.received_messages.append(data.decode("utf-8"))
            writer.close()
            await writer.wait_closed()

        server = await asyncio.start_unix_server(handle_client, self.socket_path)

        async with server:
            await server.serve_forever()

    def test_async_connection_to_nonexistent_socket(self):
        """Test async connect to non-existent socket raises error."""

        async def test():
            with self.assertRaises(krill.ConnectionError):
                await krill.AsyncKrillClient.connect("test", "/nonexistent/path.sock")

        asyncio.run(test())

    def test_async_heartbeat(self):
        """Test async heartbeat sends correct JSON."""

        async def test():
            server_task = asyncio.create_task(self.mock_server())
            await asyncio.sleep(0.1)  # Let server start

            client = await krill.AsyncKrillClient.connect(
                "async-service", self.socket_path
            )
            await client.heartbeat()
            await client.close()

            server_task.cancel()
            try:
                await server_task
            except asyncio.CancelledError:
                pass

            self.assertTrue(len(self.received_messages) > 0)
            message = json.loads(self.received_messages[0])
            self.assertEqual(message["type"], "heartbeat")
            self.assertEqual(message["service"], "async-service")
            self.assertEqual(message["status"], "healthy")

        asyncio.run(test())

    def test_async_context_manager(self):
        """Test async client as context manager."""

        async def test():
            server_task = asyncio.create_task(self.mock_server())
            await asyncio.sleep(0.1)

            async with await krill.AsyncKrillClient.connect(
                "test", self.socket_path
            ) as client:
                await client.heartbeat()

            server_task.cancel()
            try:
                await server_task
            except asyncio.CancelledError:
                pass

            self.assertTrue(len(self.received_messages) > 0)

        asyncio.run(test())

    def test_async_multiple_heartbeats(self):
        """Test sending multiple heartbeats async."""

        async def test():
            server_task = asyncio.create_task(self.mock_server())
            await asyncio.sleep(0.1)

            client = await krill.AsyncKrillClient.connect("multi", self.socket_path)
            await client.heartbeat()
            await client.heartbeat_with_metadata({"count": "1"})
            await client.report_degraded("test reason")
            await client.report_healthy()
            await client.close()

            server_task.cancel()
            try:
                await server_task
            except asyncio.CancelledError:
                pass

            # Should have received all messages
            lines = self.received_messages[0].strip().split("\n")
            self.assertEqual(len(lines), 4)

            # Check first and third message
            msg1 = json.loads(lines[0])
            self.assertEqual(msg1["status"], "healthy")

            msg3 = json.loads(lines[2])
            self.assertEqual(msg3["status"], "degraded")
            self.assertEqual(msg3["metadata"]["reason"], "test reason")

        asyncio.run(test())


class TestErrorClasses(unittest.TestCase):
    """Test error classes."""

    def test_krill_error_is_exception(self):
        """Test that KrillError is an Exception."""
        self.assertTrue(issubclass(krill.KrillError, Exception))

    def test_connection_error_message(self):
        """Test ConnectionError has descriptive message."""
        err = krill.ConnectionError("test message")
        self.assertEqual(str(err), "test message")

    def test_send_error_message(self):
        """Test SendError has descriptive message."""
        err = krill.SendError("send failed")
        self.assertEqual(str(err), "send failed")


if __name__ == "__main__":
    unittest.main()
