// contracts/test: prove both Phase 0 deployment units create non-empty runtime bytecode.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.35;

import {BSNS} from "../src/BSNS.sol";
import {Bridge} from "../src/Bridge.sol";

contract ScaffoldTest {
    function testDeploysBSNS() public {
        BSNS token = new BSNS();
        assert(address(token).code.length > 0);
    }

    function testDeploysBridge() public {
        Bridge bridge = new Bridge();
        assert(address(bridge).code.length > 0);
    }
}
