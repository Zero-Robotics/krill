// Krill C++ SDK - Header-only client library for heartbeats
// SPDX-License-Identifier: Apache-2.0

#ifndef KRILL_HPP
#define KRILL_HPP

#include <string>
#include <map>
#include <stdexcept>
#include <sstream>
#include <cstring>

#ifdef _WIN32
#error "Krill SDK only supports Unix-like systems"
#else
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#endif

namespace krill {

class KrillError : public std::runtime_error {
public:
    explicit KrillError(const std::string& message)
        : std::runtime_error(message) {}
};

class Client {
public:
    explicit Client(const std::string& service_name,
                   const std::string& socket_path = "/tmp/krill.sock")
        : service_name_(service_name), socket_fd_(-1) {
        connect(socket_path);
    }

    ~Client() {
        if (socket_fd_ >= 0) {
            close(socket_fd_);
        }
    }

    // Delete copy constructor and assignment operator
    Client(const Client&) = delete;
    Client& operator=(const Client&) = delete;

    /// Send a heartbeat indicating the service is healthy
    void heartbeat() {
        send_heartbeat("healthy", {});
    }

    /// Send a heartbeat with custom metadata
    void heartbeat_with_metadata(const std::map<std::string, std::string>& metadata) {
        send_heartbeat("healthy", metadata);
    }

    /// Report degraded status with a reason
    void report_degraded(const std::string& reason) {
        std::map<std::string, std::string> metadata;
        metadata["reason"] = reason;
        send_heartbeat("degraded", metadata);
    }

    /// Report healthy status
    void report_healthy() {
        send_heartbeat("healthy", {});
    }

private:
    std::string service_name_;
    int socket_fd_;

    void connect(const std::string& socket_path) {
        socket_fd_ = socket(AF_UNIX, SOCK_STREAM, 0);
        if (socket_fd_ < 0) {
            throw KrillError("Failed to create socket: " + std::string(std::strerror(errno)));
        }

        struct sockaddr_un addr;
        std::memset(&addr, 0, sizeof(addr));
        addr.sun_family = AF_UNIX;
        
        if (socket_path.length() >= sizeof(addr.sun_path)) {
            close(socket_fd_);
            throw KrillError("Socket path too long");
        }
        
        std::strncpy(addr.sun_path, socket_path.c_str(), sizeof(addr.sun_path) - 1);

        if (::connect(socket_fd_, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
            close(socket_fd_);
            throw KrillError("Failed to connect to daemon: " + std::string(std::strerror(errno)));
        }
    }

    void send_heartbeat(const std::string& status,
                       const std::map<std::string, std::string>& metadata) {
        // Build JSON message
        std::ostringstream json;
        json << "{\"type\":\"heartbeat\",\"service\":\"" << escape_json(service_name_) << "\"";
        json << ",\"status\":\"" << status << "\"";
        json << ",\"metadata\":{";
        
        bool first = true;
        for (const auto& pair : metadata) {
            if (!first) json << ",";
            json << "\"" << escape_json(pair.first) << "\":\"" 
                 << escape_json(pair.second) << "\"";
            first = false;
        }
        
        json << "}}\n";

        std::string message = json.str();
        ssize_t bytes_sent = send(socket_fd_, message.c_str(), message.length(), 0);
        
        if (bytes_sent < 0) {
            throw KrillError("Failed to send heartbeat: " + std::string(std::strerror(errno)));
        }
    }

    std::string escape_json(const std::string& input) {
        std::ostringstream output;
        for (char c : input) {
            switch (c) {
                case '"':  output << "\\\""; break;
                case '\\': output << "\\\\"; break;
                case '\n': output << "\\n"; break;
                case '\r': output << "\\r"; break;
                case '\t': output << "\\t"; break;
                default:   output << c; break;
            }
        }
        return output.str();
    }
};

} // namespace krill

#endif // KRILL_HPP
