// Example usage of Krill C++ SDK

#include "krill.hpp"
#include <iostream>
#include <thread>
#include <chrono>

int main() {
    try {
        // Create a client for this service
        krill::Client client("vision-pipeline");
        
        std::cout << "Starting vision pipeline heartbeat loop..." << std::endl;
        
        // Main processing loop
        for (int i = 0; i < 10; i++) {
            // Simulate work
            std::this_thread::sleep_for(std::chrono::seconds(1));
            
            // Send heartbeat
            if (i % 3 == 0) {
                // Every 3rd iteration, send with metadata
                std::map<std::string, std::string> metadata;
                metadata["frame_count"] = std::to_string(i * 30);
                metadata["fps"] = "29.7";
                client.heartbeat_with_metadata(metadata);
                std::cout << "Sent heartbeat with metadata (iteration " << i << ")" << std::endl;
            } else {
                client.heartbeat();
                std::cout << "Sent heartbeat (iteration " << i << ")" << std::endl;
            }
        }
        
        // Simulate degraded state
        std::cout << "Simulating degraded state..." << std::endl;
        client.report_degraded("High latency detected");
        std::this_thread::sleep_for(std::chrono::seconds(2));
        
        // Recover
        std::cout << "Recovered to healthy state" << std::endl;
        client.report_healthy();
        
        std::cout << "Example complete!" << std::endl;
        
    } catch (const krill::KrillError& e) {
        std::cerr << "Krill error: " << e.what() << std::endl;
        return 1;
    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
    
    return 0;
}
