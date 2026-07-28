pragma solidity 0.8.36;
contract InclusiveDeadlineRejected {
    function rejectsExactDeadline(uint256 deadline) external pure {
        uint256 timestamp = deadline;
        assert(timestamp < deadline);
    }
}
