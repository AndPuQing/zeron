import UIKit

enum TranscriptImageReadResult {
    case loaded(UIImage?)
    case cancelled
}

/// Bounded reads shared by image key, with independently cancellable consumers.
@MainActor
final class TranscriptImageReads {
    private final class Job {
        let key: String
        let operation: @MainActor () async -> UIImage?
        var consumers: [UUID: (TranscriptImageReadResult) -> Void] = [:]
        var task: Task<Void, Never>?

        init(key: String, operation: @escaping @MainActor () async -> UIImage?) {
            self.key = key
            self.operation = operation
        }
    }

    private let limit: Int
    private var active = 0
    private var jobs: [String: Job] = [:]
    private var pending: [Job] = []

    init(limit: Int = 4) { precondition(limit > 0); self.limit = limit }

    func request(key: String, consumer: UUID,
                 operation: @escaping @MainActor () async -> UIImage?,
                 completion: @escaping (TranscriptImageReadResult) -> Void) {
        let job: Job
        if let existing = jobs[key] { job = existing }
        else {
            job = Job(key: key, operation: operation)
            jobs[key] = job
            pending.append(job)
        }
        job.consumers[consumer] = completion
        pump()
    }

    func cancel(key: String, consumer: UUID) {
        guard let job = jobs[key], let completion = job.consumers.removeValue(forKey: consumer) else { return }
        if job.consumers.isEmpty {
            jobs[key] = nil
            pending.removeAll { $0 === job }
            job.task?.cancel()
        }
        completion(.cancelled)
        pump()
    }

    private func pump() {
        while active < limit && !pending.isEmpty {
            let job = pending.removeFirst()
            active += 1
            job.task = Task { [self] in
                // Cancellation may precede the first execution of this task.
                let image: UIImage?
                if Task.isCancelled { image = nil }
                else { image = await job.operation() }
                let result: TranscriptImageReadResult = Task.isCancelled ? .cancelled : .loaded(image)
                active -= 1
                if jobs[job.key] === job { jobs[job.key] = nil }
                job.task = nil
                let consumers = Array(job.consumers.values)
                job.consumers.removeAll()
                for completion in consumers { completion(result) }
                pump()
            }
        }
    }
}
