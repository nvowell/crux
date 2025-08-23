import Foundation
import SharedTypes
import UIKit

@MainActor
class Core: ObservableObject {
    @Published var view: ViewModel

    init() {
        view = try! .bincodeDeserialize(input: [UInt8](CatFacts.view()))
    }

    func update(_ event: Event) {
        let effects = [UInt8](processEvent(Data(try! event.bincodeSerialize())))

        let requests: [Request] = try! .bincodeDeserialize(input: effects)
        for request in requests {
            processEffect(request)
        }
    }
    
    private var timer: Timer?
    func clearTimer() {
        timer?.invalidate()
        timer = nil
    }
    func startTimer(durationNanos: UInt64, callback: @escaping () -> Void) {
        clearTimer()
        timer = Timer.scheduledTimer(
            withTimeInterval: TimeInterval(durationNanos) / 1_000_000_000,
            repeats: true,
        ) { _ in
            callback()
        }
    }
    
    func processEffect(_ request: Request) {
        switch request.effect {
        case .render:
            view = try! .bincodeDeserialize(input: [UInt8](CatFacts.view()))
        case let .http(req):
            Task {
                let response = try! await requestHttp(req).get()
                
                let effects = [UInt8](
                    handleResponse(
                        request.id,
                        Data(try! HttpResult.ok(response).bincodeSerialize())
                    )
                )
                
                let requests: [Request] = try! .bincodeDeserialize(input: effects)
                for request in requests {
                    processEffect(request)
                }
            }
        case let .time(req):
            switch req {
            case .now:
                let now = Date().timeIntervalSince1970;
                let response =  TimeResponse.now(instant: Instant(seconds: UInt64(now), nanos: 0))
                let effects = [UInt8](handleResponse(request.id, Data(try! response.bincodeSerialize())))
                
                let requests: [Request] = try! .bincodeDeserialize(input: effects)
                for request in requests {
                    processEffect(request)
                }
            case .interval(let id, let duration):
                startTimer(durationNanos: duration.nanos) { [weak self] in
                    let now = Date().timeIntervalSince1970
                    let instant = Instant(seconds: UInt64(now), nanos: 0)
                    let response =  TimeResponse.tick(id: id, instant: instant)
                    let effects = [UInt8](handleResponse(request.id, Data(try! response.bincodeSerialize())))
                    
                    let requests: [Request] = try! .bincodeDeserialize(input: effects)
                    for request in requests {
                        self?.processEffect(request)
                    }
                }
            case .clear(let id):
                clearTimer()
            default:
                fatalError("unhandled time request variant")
            }
        case .platform:
            let response = PlatformResponse(value: get_platform())
            
            let effects = [UInt8](handleResponse(request.id, Data(try! response.bincodeSerialize())))
            
            let requests: [Request] = try! .bincodeDeserialize(input: effects)
            for request in requests {
                processEffect(request)
            }
        case .keyValue: ()
        }
    }
}

func get_platform() -> String {
    return UIDevice.current.systemName + " " + UIDevice.current.systemVersion
}
