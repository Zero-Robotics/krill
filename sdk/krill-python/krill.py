# Krill Python SDK - Client library for sending heartbeats to krill daemon
# SPDX-License-Identifier: Apache-2.0

"""
Krill Python SDK

Lightweight client for communicating with the Krill daemon over Unix sockets.
Zero external dependencies - uses only the Python standard library.

Supports both synchronous and asynchronous (asyncio) usage.

Usage (sync):
    from krill import KrillClient

    client = KrillClient("my-service")
    client.heartbeat()

Usage (async):
    from krill import AsyncKrillClient

    client = await AsyncKrillClient.connect("my-service")
    await client.heartbeat()
"""

from __future__ import annotations

import asyncio
import json
import socket
import threading
from typing import Dict, Optional

__all__ = ["KrillClient", "AsyncKrillClient", "KrillError"]

DEFAULT_SOCKET_PATH = "/tmp/krill.sock"


class KrillError(Exception):
    """Base exception for Krill SDK errors."""

    pass


class ConnectionError(KrillError):
    """Raised when the client cannot connect to the daemon."""

    pass


class SendError(KrillError):
    """Raised when a message fails to send."""

    pass


class KrillClient:
    """Synchronous client for sending heartbeats to the Krill daemon.

    Thread-safe: the underlying socket is protected by a lock.

    Args:
        service_name: The name of the service this client represents.
        socket_path: Path to the Krill daemon Unix socket.
    """

    def __init__(
        self,
        service_name: str,
        socket_path: str = DEFAULT_SOCKET_PATH,
    ) -> None:
        self._service_name = service_name
        self._socket_path = socket_path
        self._lock = threading.Lock()
        self._sock: Optional[socket.socket] = None
        self._connect()

    def _connect(self) -> None:
        try:
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            sock.connect(self._socket_path)
            self._sock = sock
        except OSError as exc:
            raise ConnectionError(
                f"Failed to connect to daemon at {self._socket_path}: {exc}"
            ) from exc

    def heartbeat(self) -> None:
        """Send a healthy heartbeat to the daemon."""
        self._send_heartbeat("healthy", {})

    def heartbeat_with_metadata(self, metadata: Dict[str, str]) -> None:
        """Send a healthy heartbeat with custom metadata."""
        self._send_heartbeat("healthy", metadata)

    def report_degraded(self, reason: str) -> None:
        """Report degraded status with a reason."""
        self._send_heartbeat("degraded", {"reason": reason})

    def report_healthy(self) -> None:
        """Report healthy status (alias for heartbeat)."""
        self._send_heartbeat("healthy", {})

    def close(self) -> None:
        """Close the connection to the daemon."""
        with self._lock:
            if self._sock is not None:
                try:
                    self._sock.close()
                except OSError:
                    pass
                self._sock = None

    def _send_heartbeat(self, status: str, metadata: Dict[str, str]) -> None:
        message = {
            "type": "heartbeat",
            "service": self._service_name,
            "status": status,
            "metadata": metadata,
        }
        line = json.dumps(message, separators=(",", ":")) + "\n"
        data = line.encode("utf-8")

        with self._lock:
            if self._sock is None:
                raise SendError("Not connected to daemon")
            try:
                self._sock.sendall(data)
            except OSError as exc:
                raise SendError(f"Failed to send heartbeat: {exc}") from exc

    def __enter__(self) -> KrillClient:
        return self

    def __exit__(self, *args: object) -> None:
        self.close()

    def __del__(self) -> None:
        self.close()


class AsyncKrillClient:
    """Asynchronous client for sending heartbeats to the Krill daemon.

    Uses asyncio for non-blocking I/O. Not thread-safe - use from a single
    asyncio event loop.

    Use the ``connect`` classmethod to create instances:

        client = await AsyncKrillClient.connect("my-service")
    """

    def __init__(
        self,
        service_name: str,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        self._service_name = service_name
        self._reader = reader
        self._writer = writer

    @classmethod
    async def connect(
        cls,
        service_name: str,
        socket_path: str = DEFAULT_SOCKET_PATH,
    ) -> AsyncKrillClient:
        """Connect to the Krill daemon.

        Args:
            service_name: The name of the service this client represents.
            socket_path: Path to the Krill daemon Unix socket.

        Returns:
            A connected AsyncKrillClient instance.

        Raises:
            ConnectionError: If the connection fails.
        """
        try:
            reader, writer = await asyncio.open_unix_connection(socket_path)
        except OSError as exc:
            raise ConnectionError(
                f"Failed to connect to daemon at {socket_path}: {exc}"
            ) from exc
        return cls(service_name, reader, writer)

    async def heartbeat(self) -> None:
        """Send a healthy heartbeat to the daemon."""
        await self._send_heartbeat("healthy", {})

    async def heartbeat_with_metadata(self, metadata: Dict[str, str]) -> None:
        """Send a healthy heartbeat with custom metadata."""
        await self._send_heartbeat("healthy", metadata)

    async def report_degraded(self, reason: str) -> None:
        """Report degraded status with a reason."""
        await self._send_heartbeat("degraded", {"reason": reason})

    async def report_healthy(self) -> None:
        """Report healthy status."""
        await self._send_heartbeat("healthy", {})

    async def close(self) -> None:
        """Close the connection to the daemon."""
        try:
            self._writer.close()
            await self._writer.wait_closed()
        except OSError:
            pass

    async def _send_heartbeat(self, status: str, metadata: Dict[str, str]) -> None:
        message = {
            "type": "heartbeat",
            "service": self._service_name,
            "status": status,
            "metadata": metadata,
        }
        line = json.dumps(message, separators=(",", ":")) + "\n"
        data = line.encode("utf-8")

        try:
            self._writer.write(data)
            await self._writer.drain()
        except OSError as exc:
            raise SendError(f"Failed to send heartbeat: {exc}") from exc

    async def __aenter__(self) -> AsyncKrillClient:
        return self

    async def __aexit__(self, *args: object) -> None:
        await self.close()
